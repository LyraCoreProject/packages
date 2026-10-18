use std::collections::BTreeSet;

use super::catalog::Role;

pub(crate) const BOT_WAIT_MICROS: i64 = 60_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueueParty {
    pub members: Vec<(u64, Role)>,
    pub queued_micros: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MatchRoster {
    pub humans: Vec<u64>,
    pub bot_roles: Vec<Role>,
}

fn consume(open: &mut Vec<Role>, members: &[(u64, Role)]) -> bool {
    let mut remaining = open.clone();
    for (_, role) in members {
        let Some(index) = remaining.iter().position(|r| r == role) else {
            return false;
        };
        remaining.remove(index);
    }
    *open = remaining;
    true
}

fn roles() -> Vec<Role> {
    vec![
        Role::Tank,
        Role::Healer,
        Role::Damage,
        Role::Damage,
        Role::Damage,
    ]
}

pub(crate) fn can_queue(members: &[(u64, Role)]) -> bool {
    !members.is_empty()
        && members.len() <= 5
        && members.iter().all(|(guid, _)| *guid != 0)
        && members
            .iter()
            .map(|(guid, _)| guid)
            .collect::<BTreeSet<_>>()
            .len()
            == members.len()
        && consume(&mut roles(), members)
}

pub(crate) fn next_match(parties: &[QueueParty], now_micros: i64) -> Option<MatchRoster> {
    let mut waiting: Vec<_> = parties.iter().filter(|p| can_queue(&p.members)).collect();
    waiting.sort_by_key(|p| (p.queued_micros, p.members[0].0));
    let oldest = waiting.first()?;
    let mut open = roles();
    let mut humans = Vec::new();
    for party in waiting.iter().take(256) {
        if party.members.iter().any(|(guid, _)| humans.contains(guid)) {
            continue;
        }
        if consume(&mut open, &party.members) {
            humans.extend(party.members.iter().map(|(guid, _)| *guid));
        }
        if open.is_empty() {
            break;
        }
    }
    if !open.is_empty() && now_micros.saturating_sub(oldest.queued_micros) < BOT_WAIT_MICROS {
        return None;
    }
    Some(MatchRoster {
        humans,
        bot_roles: open,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn party(members: &[(u64, Role)], queued_micros: i64) -> QueueParty {
        QueueParty {
            members: members.to_vec(),
            queued_micros,
        }
    }

    #[test]
    fn five_humans_start_without_waiting_for_bots() {
        let parties = [
            party(&[(1, Role::Damage), (2, Role::Healer)], 1),
            party(&[(3, Role::Tank)], 2),
            party(&[(4, Role::Damage), (5, Role::Damage)], 3),
        ];
        assert_eq!(
            next_match(&parties, 4),
            Some(MatchRoster {
                humans: vec![1, 2, 3, 4, 5],
                bot_roles: vec![],
            })
        );
    }

    #[test]
    fn solo_damage_waits_a_minute_then_gets_the_missing_roles() {
        let parties = [party(&[(1, Role::Damage)], 10)];
        assert_eq!(next_match(&parties, 60_000_009), None);
        assert_eq!(
            next_match(&parties, 60_000_010),
            Some(MatchRoster {
                humans: vec![1],
                bot_roles: vec![Role::Tank, Role::Healer, Role::Damage, Role::Damage],
            })
        );
    }

    #[test]
    fn a_premade_is_never_split_to_fill_the_oldest_party() {
        let parties = [
            party(
                &[
                    (1, Role::Tank),
                    (2, Role::Healer),
                    (3, Role::Damage),
                    (4, Role::Damage),
                ],
                0,
            ),
            party(&[(5, Role::Damage), (6, Role::Damage)], 1),
        ];
        assert_eq!(
            next_match(&parties, BOT_WAIT_MICROS),
            Some(MatchRoster {
                humans: vec![1, 2, 3, 4],
                bot_roles: vec![Role::Damage],
            })
        );
    }

    #[test]
    fn incompatible_or_duplicate_members_cannot_queue() {
        assert!(!can_queue(&[(1, Role::Tank), (2, Role::Tank)]));
        assert!(!can_queue(&[(1, Role::Tank), (1, Role::Healer)]));
        assert!(!can_queue(&[]));
        assert!(!can_queue(&[(0, Role::Damage)]));
    }
}
