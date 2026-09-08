//! Human-led party facts and the Priest's follow and heal Strategies.

use super::decision::{
    Action, ActionNode, CastAction, MoveTarget, Readiness, Reason, Strategy, Trigger,
};
use super::{
    pkg_playerbots_bot, pkg_playerbots_personality, PlayerbotsBot, PlayerbotsRotation, ROLE_HEALER,
};
use spacetimedb::ReducerContext;

pub(super) struct Party {
    pub leader_guid: u64,
    pub leader: Option<crate::group::PartyUnitFacts>,
    pub members: Vec<crate::group::PartyMemberFacts>,
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
        leader_guid: facts.leader_guid,
        leader,
        members: facts.members,
    }))
}

fn distance_sq(me: &crate::WorldEntity, unit: &crate::group::PartyUnitFacts) -> f32 {
    (me.x - unit.x).powi(2) + (me.y - unit.y).powi(2) + (me.z - unit.z).powi(2)
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

pub(super) struct CompanionSelection {
    pub strategy: Strategy,
    pub heal_target: Option<u64>,
}

pub(super) fn strategy(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    party: &Party,
    heal_spell: Option<&PlayerbotsRotation>,
    survival_permits_healing: bool,
    objective: u64,
    retained_heal_target: Option<u64>,
) -> CompanionSelection {
    let follow = follow(party, me, objective);
    let personality_heal_at = ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(me.guid)
        .next()
        .map_or(u8::MAX, |personality| personality.heal_at_pct);
    let heal_at_pct = heal_spell.map_or(0, |spell| spell.threshold_pct.min(personality_heal_at));
    let heal_target = if bot.role == ROLE_HEALER {
        heal_spell.and_then(|_| {
            retained_heal_target
                .and_then(|target| {
                    wounded_ally(
                        party,
                        (me.map_id, me.instance_id),
                        heal_at_pct,
                        Some(target),
                    )
                })
                .or_else(|| {
                    if survival_permits_healing {
                        wounded_ally(party, (me.map_id, me.instance_id), heal_at_pct, None)
                    } else {
                        None
                    }
                })
        })
    } else {
        None
    };
    let heal = if survival_permits_healing {
        heal_spell.and_then(|spell| {
            let target = heal_target?;
            let cast = CastAction {
                target,
                spell: spell.spell_id,
            };
            let mut heal = node(Action::Cast(cast), Reason::Heal, 800, objective);
            match crate::actor::cast_readiness(ctx, me.guid, spell.spell_id, target) {
                Ok(()) => {}
                Err(refusal)
                    if matches!(
                        refusal.kind,
                        crate::spell::CastRefusalKind::OutOfRange
                            | crate::spell::CastRefusalKind::NoLineOfSight
                    ) =>
                {
                    let mut position = node(
                        Action::Move(MoveTarget::CastingPosition(target)),
                        Reason::CastingPosition,
                        800,
                        objective,
                    );
                    position.alternatives.push(node(
                        Action::Hold,
                        Reason::CastingPosition,
                        800,
                        objective,
                    ));
                    heal.prerequisites.push(position);
                }
                Err(_) => heal.readiness = Readiness::Refused,
            }
            heal.alternatives.push(follow.clone());
            Some(heal)
        })
    } else {
        None
    };
    let mut candidates = Vec::with_capacity(2);
    if let Some(heal) = heal {
        candidates.push(heal);
    }
    candidates.push(follow);
    CompanionSelection {
        strategy: Strategy {
            trigger: Trigger::Always,
            candidates,
            defaults: vec![],
            priority_adjustment: 0,
        },
        heal_target,
    }
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
            }),
        }
    }

    #[test]
    fn the_most_injured_ally_wins_with_guid_as_the_stable_tie_break() {
        let party = Party {
            leader_guid: 10,
            leader: None,
            members: vec![member(12, 20, 100), member(11, 10, 50), member(13, 30, 100)],
        };
        assert_eq!(wounded_ally(&party, (0, 0), 50, None), Some(11));
        assert_eq!(wounded_ally(&party, (0, 0), 50, Some(12)), Some(12));
    }
}
