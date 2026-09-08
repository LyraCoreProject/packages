//! Pure, bounded selection from typed Strategies and explicit facts.

use crate::nav::LEG_MAX_EXPANSIONS;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Hold,
    Move(MoveTarget),
    Cast(CastAction),
    Attack(u64),
    Resurrect,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CastAction {
    pub target: u64,
    pub spell: u32,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MoveTarget {
    Home,
    Entity(u64),
    CastingPosition(u64),
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    Restricted,
    Survival,
    Recovery,
    Defense,
    ReturnHome,
    Idle,
    Follow,
    Heal,
    CastingPosition,
    Resurrection,
    PartyUnavailable,
    Provisioning,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CandidateId {
    pub action: Action,
    pub reason: Reason,
    pub objective: u64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: CandidateId,
    pub priority: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Complete,
    Refused,
    NotBefore(i64),
}

#[derive(Clone, Debug)]
pub struct ActionNode {
    pub candidate: Candidate,
    pub readiness: Readiness,
    pub prerequisites: Vec<ActionNode>,
    pub alternatives: Vec<ActionNode>,
    pub continuers: Vec<ActionNode>,
}

impl ActionNode {
    pub fn ready(action: Action, reason: Reason, priority: i32) -> Self {
        Self {
            candidate: Candidate {
                id: CandidateId {
                    action,
                    reason,
                    objective: 0,
                },
                priority,
            },
            readiness: Readiness::Ready,
            prerequisites: Vec::new(),
            alternatives: Vec::new(),
            continuers: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Trigger {
    Restricted,
    LowHealth,
    Wounded,
    Attacked,
    Away,
    Always,
}

pub struct Facts {
    pub now: i64,
    pub restricted: bool,
    pub low_health: bool,
    pub wounded: bool,
    pub attacked: bool,
    pub away: bool,
}

impl Trigger {
    fn active(self, facts: &Facts) -> bool {
        match self {
            Self::Restricted => facts.restricted,
            Self::LowHealth => facts.low_health,
            Self::Wounded => facts.wounded,
            Self::Attacked => facts.attacked,
            Self::Away => facts.away,
            Self::Always => true,
        }
    }
}

pub struct Strategy {
    pub trigger: Trigger,
    pub candidates: Vec<ActionNode>,
    pub defaults: Vec<ActionNode>,
    pub priority_adjustment: i32,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub candidates: usize,
    pub depth: usize,
    pub transitions: usize,
    pub route_expansions: u32,
}

pub const LIMITS: Limits = Limits {
    candidates: 24,
    depth: 4,
    transitions: 16,
    route_expansions: LEG_MAX_EXPANSIONS,
};

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionRefusal {
    Candidates,
    Depth,
    Transitions,
    RouteWork,
    Cycle,
    Unavailable,
}

pub struct Decision {
    pub order: Vec<Candidate>,
    pub chosen: Option<Candidate>,
    pub refusals: Vec<DecisionRefusal>,
    pub transitions: usize,
    pub route_expansions: u32,
}

struct Search {
    limits: Limits,
    visited: usize,
    transitions: usize,
    route_expansions: u32,
    refusals: Vec<DecisionRefusal>,
    refusal_count: usize,
}

impl Search {
    fn refuse(&mut self, reason: DecisionRefusal) {
        self.refusal_count += 1;
        if !self.refusals.contains(&reason) {
            self.refusals.push(reason);
        }
    }

    fn resolve(
        &mut self,
        node: &ActionNode,
        now: i64,
        path: &mut Vec<CandidateId>,
    ) -> Option<Candidate> {
        if self.visited >= self.limits.candidates {
            self.refuse(DecisionRefusal::Candidates);
            return None;
        }
        self.visited += 1;
        if self.transitions >= self.limits.transitions {
            self.refuse(DecisionRefusal::Transitions);
            return None;
        }
        self.transitions += 1;
        if path.contains(&node.candidate.id) {
            self.refuse(DecisionRefusal::Cycle);
            return None;
        }
        if path.len() >= self.limits.depth {
            self.refuse(DecisionRefusal::Depth);
            return None;
        }
        path.push(node.candidate.id);
        let chosen = match node.readiness {
            Readiness::Complete => self.first(&node.continuers, now, path),
            Readiness::Refused => self.first(&node.alternatives, now, path),
            Readiness::NotBefore(at) if at > now => self.first(&node.alternatives, now, path),
            Readiness::Ready | Readiness::NotBefore(_) => {
                let mut pending = None;
                let mut refused = false;
                for prerequisite in &node.prerequisites {
                    let refusals_before = self.refusal_count;
                    pending = self.resolve(prerequisite, now, path);
                    if pending.is_none()
                        && prerequisite.readiness == Readiness::Complete
                        && self.refusal_count == refusals_before
                    {
                        continue;
                    }
                    refused = pending.is_none();
                    break;
                }
                if refused {
                    self.first(&node.alternatives, now, path)
                } else if let Some(mut candidate) = pending {
                    candidate.priority = candidate.priority.max(node.candidate.priority);
                    Some(candidate)
                } else if matches!(node.candidate.id.action, Action::Move(_))
                    && self.limits.route_expansions < LEG_MAX_EXPANSIONS
                {
                    self.refuse(DecisionRefusal::RouteWork);
                    self.first(&node.alternatives, now, path)
                } else {
                    if matches!(node.candidate.id.action, Action::Move(_)) {
                        self.route_expansions = LEG_MAX_EXPANSIONS;
                    }
                    Some(node.candidate)
                }
            }
        };
        path.pop();
        chosen
    }

    fn first(
        &mut self,
        nodes: &[ActionNode],
        now: i64,
        path: &mut Vec<CandidateId>,
    ) -> Option<Candidate> {
        for node in nodes {
            if let Some(candidate) = self.resolve(node, now, path) {
                return Some(candidate);
            }
            if self.visited >= self.limits.candidates || self.transitions >= self.limits.transitions
            {
                break;
            }
        }
        None
    }
}

/// Candidate identity includes its target, spell and trigger. Equal facts and time have no random input.
/// Limits apply across all active Strategies, including failed prerequisite and fallback branches.
pub fn choose(facts: &Facts, strategies: &[Strategy], limits: Limits) -> Decision {
    let mut roots = Vec::new();
    let mut overflow = false;
    'strategies: for strategy in strategies {
        let nodes = if strategy.trigger.active(facts) {
            &strategy.candidates
        } else {
            &strategy.defaults
        };
        for node in nodes {
            if roots.len() == limits.candidates {
                overflow = true;
                break 'strategies;
            }
            roots.push((
                node,
                node.candidate
                    .priority
                    .saturating_add(strategy.priority_adjustment),
            ));
        }
    }
    roots.sort_by_key(|(node, priority)| (std::cmp::Reverse(*priority), node.candidate.id));
    let mut identities = std::collections::BTreeSet::new();
    roots.retain(|(node, _)| identities.insert(node.candidate.id));
    let order = roots
        .iter()
        .map(|(node, priority)| Candidate {
            priority: *priority,
            ..node.candidate
        })
        .collect();
    let mut search = Search {
        limits,
        visited: 0,
        transitions: 0,
        route_expansions: 0,
        refusals: Vec::new(),
        refusal_count: 0,
    };
    if overflow {
        search.refuse(DecisionRefusal::Candidates);
    }
    let mut chosen = None;
    for (node, priority) in roots {
        if let Some(mut candidate) = search.resolve(node, facts.now, &mut Vec::new()) {
            candidate.priority = priority;
            chosen = Some(candidate);
            break;
        }
        if search.visited >= limits.candidates || search.transitions >= limits.transitions {
            break;
        }
    }
    if chosen.is_none() {
        search.refuse(DecisionRefusal::Unavailable);
    }
    Decision {
        order,
        chosen,
        refusals: search.refusals,
        transitions: search.transitions,
        route_expansions: search.route_expansions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            now: 100,
            restricted: false,
            low_health: false,
            wounded: false,
            attacked: true,
            away: true,
        }
    }
    fn strategy(nodes: Vec<ActionNode>) -> Strategy {
        Strategy {
            trigger: Trigger::Always,
            candidates: nodes,
            defaults: vec![],
            priority_adjustment: 0,
        }
    }
    fn attack(target: u64) -> ActionNode {
        ActionNode::ready(Action::Attack(target), Reason::Defense, 50)
    }

    #[test]
    fn fixed_facts_rank_targets_and_priorities_independent_of_input_order() {
        for targets in [[22, 11], [11, 22]] {
            let result = choose(
                &facts(),
                &[strategy(targets.into_iter().map(attack).collect())],
                LIMITS,
            );
            assert_eq!(
                result.order.iter().map(|c| c.id.action).collect::<Vec<_>>(),
                vec![Action::Attack(11), Action::Attack(22)]
            );
            assert_eq!(result.chosen.unwrap().id.action, Action::Attack(11));
        }
    }

    #[test]
    fn strategies_compose_triggers_defaults_and_priority() {
        let mut defense = strategy(vec![attack(11)]);
        defense.priority_adjustment = 40;
        let survival = Strategy {
            trigger: Trigger::LowHealth,
            candidates: vec![ActionNode::ready(
                Action::Move(MoveTarget::Home),
                Reason::Survival,
                100,
            )],
            defaults: vec![ActionNode::ready(Action::Hold, Reason::Idle, 1)],
            priority_adjustment: 0,
        };
        let mut f = facts();
        let strategies = [defense, survival];
        assert_eq!(
            choose(&f, &strategies, LIMITS).chosen.unwrap().id.action,
            Action::Attack(11)
        );
        f.low_health = true;
        assert_eq!(
            choose(&f, &strategies, LIMITS).chosen.unwrap().id.reason,
            Reason::Survival
        );
    }

    #[test]
    fn prerequisites_keep_the_target_and_continuers_require_completion() {
        let mut node = attack(22);
        node.prerequisites.push(ActionNode::ready(
            Action::Move(MoveTarget::Entity(22)),
            Reason::Defense,
            1,
        ));
        let result = choose(&facts(), &[strategy(vec![node.clone()])], LIMITS);
        assert_eq!(
            result.chosen.unwrap(),
            Candidate {
                id: CandidateId {
                    action: Action::Move(MoveTarget::Entity(22)),
                    reason: Reason::Defense,
                    objective: 0
                },
                priority: 50
            }
        );
        node.prerequisites[0].readiness = Readiness::Complete;
        node.continuers.push(ActionNode::ready(
            Action::Move(MoveTarget::Home),
            Reason::ReturnHome,
            10,
        ));
        assert_eq!(
            choose(&facts(), &[strategy(vec![node.clone()])], LIMITS)
                .chosen
                .unwrap()
                .id
                .action,
            Action::Attack(22)
        );
        node.readiness = Readiness::Complete;
        assert_eq!(
            choose(&facts(), &[strategy(vec![node])], LIMITS)
                .chosen
                .unwrap()
                .id
                .reason,
            Reason::ReturnHome
        );
    }

    #[test]
    fn repeated_cycle_refusals_cannot_satisfy_a_completed_prerequisite() {
        let mut first = attack(1);
        first.readiness = Readiness::Complete;
        first.candidate.priority = 100;
        first.continuers.push(attack(1));
        let mut prerequisite = attack(3);
        prerequisite.readiness = Readiness::Complete;
        prerequisite.continuers.push(attack(3));
        let mut second = attack(2);
        second.prerequisites.push(prerequisite);
        let result = choose(&facts(), &[strategy(vec![first, second])], LIMITS);
        assert!(result.chosen.is_none());
        assert!(result.refusals.contains(&DecisionRefusal::Cycle));
    }

    #[test]
    fn cycles_refuse_boundedly() {
        let mut cycle = attack(11);
        cycle.prerequisites.push(attack(11));
        assert!(choose(&facts(), &[strategy(vec![cycle])], LIMITS)
            .refusals
            .contains(&DecisionRefusal::Cycle));
    }

    #[test]
    fn each_work_limit_refuses_boundedly() {
        for (limits, refusal) in [
            (
                Limits {
                    candidates: 0,
                    ..LIMITS
                },
                DecisionRefusal::Candidates,
            ),
            (Limits { depth: 0, ..LIMITS }, DecisionRefusal::Depth),
            (
                Limits {
                    transitions: 0,
                    ..LIMITS
                },
                DecisionRefusal::Transitions,
            ),
            (
                Limits {
                    route_expansions: 0,
                    ..LIMITS
                },
                DecisionRefusal::RouteWork,
            ),
        ] {
            let result = choose(
                &facts(),
                &[strategy(vec![ActionNode::ready(
                    Action::Move(MoveTarget::Home),
                    Reason::ReturnHome,
                    1,
                )])],
                limits,
            );
            assert!(result.chosen.is_none());
            assert!(result.refusals.contains(&refusal));
        }
    }

    #[test]
    fn refused_actions_use_alternatives_and_time_is_explicit() {
        let mut node = attack(11);
        node.readiness = Readiness::Refused;
        let mut fallback = ActionNode::ready(Action::Hold, Reason::Idle, 1);
        fallback.readiness = Readiness::NotBefore(101);
        node.alternatives.push(fallback);
        let strategies = [strategy(vec![node])];
        assert!(choose(&facts(), &strategies, LIMITS).chosen.is_none());
        let mut later = facts();
        later.now = 101;
        assert_eq!(
            choose(&later, &strategies, LIMITS)
                .chosen
                .unwrap()
                .id
                .action,
            Action::Hold
        );
    }
    #[test]
    fn a_candidate_keeps_only_its_highest_priority_offer() {
        let mut high = attack(11);
        high.candidate.priority = 100;
        let result = choose(
            &facts(),
            &[strategy(vec![attack(11), attack(22), high])],
            LIMITS,
        );
        assert_eq!(result.order.len(), 2);
        assert_eq!(result.order[0].priority, 100);
        assert_eq!(result.order[0].id.action, Action::Attack(11));
    }

    #[test]
    fn completed_prerequisites_still_consume_transition_work() {
        let mut node = attack(11);
        for target in 20..40 {
            let mut prerequisite = attack(target);
            prerequisite.readiness = Readiness::Complete;
            node.prerequisites.push(prerequisite);
        }
        let result = choose(&facts(), &[strategy(vec![node])], LIMITS);
        assert!(result.chosen.is_none());
        assert_eq!(result.transitions, LIMITS.transitions);
        assert!(result.refusals.contains(&DecisionRefusal::Transitions));
    }
    #[test]
    fn a_strategy_can_lower_an_offers_priority() {
        let mut defense = strategy(vec![attack(11)]);
        defense.priority_adjustment = -40;
        let result = choose(&facts(), &[defense], LIMITS);
        assert_eq!(result.order[0].priority, 10);
        assert_eq!(result.chosen.unwrap().priority, 10);
    }
}
