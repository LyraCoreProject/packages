//! Human-led party facts, target choice and follow decisions.

use super::class_behavior::{self, Fight, FightFallback, SupportContext};
use super::decision::{Action, ActionNode, MoveTarget, Reason, Strategy, Trigger};
use super::{pkg_playerbots_bot, PlayerbotsBot, ROLE_TANK};
use spacetimedb::ReducerContext;

pub use super::class_behavior::RoleReadError;

pub(super) struct Party {
    pub group_id: u64,
    pub leader_guid: u64,
    pub leader: Option<crate::group::PartyUnitFacts>,
    pub members: Vec<crate::group::PartyMemberFacts>,
    pub enemies: Vec<crate::group::PartyEnemyFacts>,
    pub fight_constraint: Option<u64>,
}

impl Party {
    pub fn leader_partition(&self) -> Option<crate::group::PartyPartitionFacts> {
        self.members
            .iter()
            .find(|member| member.character_guid == self.leader_guid)
            .and_then(|member| {
                member.partition.or_else(|| {
                    member
                        .unit
                        .as_ref()
                        .map(|unit| crate::group::PartyPartitionFacts {
                            map_id: unit.map_id,
                            instance_id: unit.instance_id,
                            locator_revision: 0,
                        })
                })
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
    party(ctx, character_guid, true)
}

pub(super) fn party(
    ctx: &ReducerContext,
    character_guid: u64,
    require_human_leader: bool,
) -> Result<Option<Party>, crate::group::PartyFactsUnavailable> {
    let Some(facts) = crate::group::party_facts(ctx, character_guid)? else {
        return Ok(None);
    };
    if require_human_leader
        && ctx
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

fn node(action: Action, reason: Reason, priority: i32, objective: u64) -> ActionNode {
    let mut node = ActionNode::ready(action, reason, priority);
    node.candidate.id.objective = objective;
    node
}

fn follow(
    party: &Party,
    member_guid: Option<u64>,
    me: &crate::WorldEntity,
    objective: u64,
) -> ActionNode {
    let Some((member_guid, member)) = member_guid.and_then(|guid| {
        party
            .members
            .iter()
            .find(|member| member.character_guid == guid)
            .and_then(|member| member.unit.as_ref())
            .map(|member| (guid, member))
    }) else {
        return node(Action::Hold, Reason::Follow, 100, objective);
    };
    if (member.map_id, member.instance_id) != (me.map_id, me.instance_id)
        || distance_sq(me, member) <= 3.05 * 3.05
    {
        return node(Action::Hold, Reason::Follow, 100, objective);
    }
    node(
        Action::Move(MoveTarget::Entity(member_guid)),
        Reason::Follow,
        100,
        objective,
    )
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

pub(super) struct CompanionSelection {
    pub strategy: Strategy,
    pub heal_target: Option<u64>,
    pub fight_target: Option<u64>,
    pub buff_target: Option<u64>,
    pub read_failure: Option<RoleReadError>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn strategy(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    party: &Party,
    follow_member_guid: Option<u64>,
    survival_permits_healing: bool,
    objective: u64,
    retained_heal_target: Option<u64>,
    retained_fight_target: Option<u64>,
    retained_buff_target: Option<u64>,
) -> Result<CompanionSelection, RoleReadError> {
    let follow = follow(party, follow_member_guid, me, objective);
    let support = class_behavior::support(
        ctx,
        bot,
        me,
        SupportContext {
            party: Some(party),
            in_combat: !party.enemies.is_empty()
                || me.combat_until_ms > (ctx.timestamp.to_micros_since_unix_epoch() / 1000) as u64,
            permits_healing: survival_permits_healing,
            heal_target: retained_heal_target,
            buff_target: retained_buff_target,
        },
        objective,
        &follow,
    )?;
    let heal_target = support.heal_target;
    let buff_target = support.buff_target;
    let read_failure = support.read_failure;
    let fight_target = fight_target(party, me, bot.role, retained_fight_target);
    let mut candidates = support.candidates;
    if let Some(target) = fight_target {
        let tank = bot.role == ROLE_TANK;
        candidates.push(class_behavior::combat(
            ctx,
            bot,
            me,
            Fight {
                target: target.guid,
                protecting_ally: tank
                    && target.attacking_party
                    && target.current_target_guid != Some(me.guid),
                tank_engaged: target.party_has_threat,
                fallback: if tank {
                    FightFallback::Melee
                } else {
                    FightFallback::Hold
                },
            },
            objective,
            if tank {
                Reason::TankFight
            } else {
                Reason::DamageFight
            },
            if tank { 700 } else { 650 },
        )?);
    } else if !party.enemies.is_empty() {
        candidates.push(node(Action::Hold, Reason::CrowdControl, 750, objective));
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
