//! Bounded provisioning for Cohort bots. Spawn and level-up only arm this state. The admitted
//! runner performs one recorded operation when it has no Foreground Action.

use super::{
    class, pkg_playerbots_bot, pkg_playerbots_kit, Controller, PlayerbotsBot, ROLE_DPS, ROLE_HEALER,
};
use crate::{game_item_instance, game_world_entity};
use spacetimedb::{table, ReducerContext, Table};

const PROFILE_REVISION: u32 = 1;
pub(super) const WARRIOR_PROFILE_SKILL: u32 = 43;
const STEP_INTERVAL_MICROS: i64 = 1_000_000;
const RETRY_INTERVAL_MICROS: i64 = 30_000_000;
const REPAIR_INTERVAL_MICROS: i64 = 60_000_000;
const HISTORY_LIMIT: usize = 32;
const MAX_PROFILE_SPELLS: usize = 12;
const MAX_ACTION_SCAN: usize = 32;

const BAG: u32 = 4496;
const FOOD: u32 = 117;
const DRINK: u32 = 159;
const POTION: u32 = 118;
const BANDAGE: u32 = 1251;
const AMMO: u32 = 2512;
const HEARTHSTONE: u32 = 6948;
const PALADIN_REAGENT: u32 = 17033;
const PRIEST_REAGENT: u32 = 17029;
const MAGE_REAGENT: u32 = 17056;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisionCause {
    Spawn,
    LevelUp,
    Periodic,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisionItemKind {
    Gear,
    Bag,
    Food,
    Drink,
    Potion,
    Ammo,
    Reagent,
    Supply,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvisionItem {
    pub kind: ProvisionItemKind,
    pub entry: u32,
    pub target_count: u32,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvisionTalent {
    pub preferred_tree: u8,
    pub talent_id: Option<u32>,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisionHold {
    Dead,
    InCombat,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisionAction {
    RecoverHealth,
    RecoverPower,
    Skill(u32),
    Spell(u32),
    Talent(ProvisionTalent),
    Item(ProvisionItem),
    Equip(ProvisionItem),
    Hold(ProvisionHold),
    Audit,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisionRefusalKind {
    MissingResource,
    InventoryFull,
    Class,
    Level,
    Prerequisite,
    ProfileLimit,
    CoreGate,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq, Eq)]
pub struct ProvisionRefusal {
    pub kind: ProvisionRefusalKind,
    pub detail: String,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisionDeferral {
    Dead,
    InCombat,
    MissingConsumable,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq, Eq)]
pub enum ProvisionOutcome {
    Applied(u32),
    Satisfied,
    Refused(ProvisionRefusal),
    Deferred(ProvisionDeferral),
    Stopped(ProvisionRefusal),
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq, Eq)]
pub struct ProvisionDecision {
    pub at_micros: i64,
    pub cause: ProvisionCause,
    pub profile: String,
    pub revision: u32,
    pub action: ProvisionAction,
    pub outcome: ProvisionOutcome,
}

/// One profile cursor and its bounded explanation history per bot. The profile explicitly records
/// that its core-admitted training and item grants are free.
#[table(accessor = pkg_playerbots_provisioning, public)]
pub struct PlayerbotsProvisioning {
    #[primary_key]
    pub character_guid: u64,
    pub profile: String,
    pub revision: u32,
    pub free_grants: bool,
    pub armed_level: u32,
    pub cause: ProvisionCause,
    pub action_cursor: u16,
    pub next_repair_micros: i64,
    pub history: Vec<ProvisionDecision>,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_provisioning(ctx, character_guid) {
    ctx.db.pkg_playerbots_provisioning().character_guid().delete(character_guid);
});
crate::character_owned!(transfer, fn sweep_transfer_pkg_playerbots_provisioning(ctx, character_guid, io) {
    table = pkg_playerbots_provisioning,
    primary_key = character_guid,
});

pub(super) enum ReconcileStep {
    Ready,
    Recorded,
    Worked,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProvisionEffect {
    Other,
    RecoveryGrant,
    RecoveryUse,
}

struct Execution {
    outcome: ProvisionOutcome,
    effect: ProvisionEffect,
}

impl Execution {
    fn other(outcome: ProvisionOutcome) -> Self {
        Self {
            outcome,
            effect: ProvisionEffect::Other,
        }
    }
}

fn role_name(role: u8) -> &'static str {
    match role {
        super::ROLE_TANK => "tank",
        ROLE_HEALER => "healer",
        _ => "damage",
    }
}

fn class_name(class: u8) -> &'static str {
    match class {
        class::WARRIOR => "warrior",
        class::PALADIN => "paladin",
        class::PRIEST => "priest",
        class::MAGE => "mage",
        _ => "unsupported",
    }
}

fn profile_name(class: u8, role: u8) -> String {
    format!("{}-{}-free", class_name(class), role_name(role))
}

pub(super) fn arm_spawn(ctx: &ReducerContext, bot: &PlayerbotsBot, level: u32, now: i64) {
    let rows = ctx.db.pkg_playerbots_provisioning();
    let row = PlayerbotsProvisioning {
        character_guid: bot.character_guid,
        profile: profile_name(bot.class, bot.role),
        revision: PROFILE_REVISION,
        free_grants: true,
        armed_level: level,
        cause: ProvisionCause::Spawn,
        action_cursor: 0,
        next_repair_micros: now,
        history: vec![],
    };
    if rows.character_guid().find(bot.character_guid).is_some() {
        rows.character_guid().update(row);
    } else {
        rows.insert(row);
    }
}

crate::game_hook!(on_levelup, fn playerbots_arm_provisioning_on_levelup(ctx, payload) {
    let Some(bot) = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(payload.character_guid)
        .next()
    else {
        return;
    };
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let rows = ctx.db.pkg_playerbots_provisioning();
    let mut state = rows
        .character_guid()
        .find(payload.character_guid)
        .unwrap_or_else(|| PlayerbotsProvisioning {
            character_guid: bot.character_guid,
            profile: profile_name(bot.class, bot.role),
            revision: PROFILE_REVISION,
            free_grants: true,
            armed_level: payload.new_level,
            cause: ProvisionCause::LevelUp,
            action_cursor: 0,
            next_repair_micros: now,
            history: vec![],
        });
    state.armed_level = payload.new_level;
    state.cause = ProvisionCause::LevelUp;
    state.action_cursor = 0;
    state.next_repair_micros = now;
    if rows.character_guid().find(payload.character_guid).is_some() {
        rows.character_guid().update(state);
    } else {
        rows.insert(state);
    }
});

fn preferred_skill(class: u8) -> u32 {
    match class {
        class::WARRIOR => WARRIOR_PROFILE_SKILL, // One-Handed Swords
        class::PALADIN | class::PRIEST => 54,    // One-Handed Maces
        class::MAGE => 136,                      // Staves
        _ => 162,                                // Unarmed
    }
}

fn preferred_talent_tree(class: u8, role: u8) -> u8 {
    match (class, role) {
        (class::WARRIOR, super::ROLE_TANK) => 2,
        (class::PALADIN, ROLE_HEALER) => 0,
        (class::PALADIN, ROLE_DPS) => 2,
        (class::PRIEST, ROLE_HEALER) => 1,
        (class::PRIEST, ROLE_DPS) => 2,
        (class::MAGE, _) => 1,
        _ => 0,
    }
}

fn weapon(class: u8) -> u32 {
    match class {
        class::PRIEST | class::PALADIN => 36,
        class::MAGE => 35,
        _ => 25,
    }
}

fn item(kind: ProvisionItemKind, entry: u32, target_count: u32) -> ProvisionItem {
    ProvisionItem {
        kind,
        entry,
        target_count,
    }
}

fn profile_limit(detail: impl Into<String>) -> ProvisionRefusal {
    ProvisionRefusal {
        kind: ProvisionRefusalKind::ProfileLimit,
        detail: detail.into(),
    }
}

pub(super) fn profile_actions(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
) -> Result<Vec<ProvisionAction>, ProvisionRefusal> {
    let gear = item(ProvisionItemKind::Gear, weapon(bot.class), 1);
    let bag = item(ProvisionItemKind::Bag, BAG, 4);
    let mut actions = vec![
        ProvisionAction::RecoverHealth,
        ProvisionAction::RecoverPower,
        ProvisionAction::Skill(preferred_skill(bot.class)),
    ];
    let mut spells: Vec<_> = ctx
        .db
        .pkg_playerbots_kit()
        .by_class_role()
        .filter((bot.class, bot.role))
        .take(MAX_PROFILE_SPELLS + 1)
        .map(|row| row.spell_id)
        .collect();
    if spells.len() > MAX_PROFILE_SPELLS {
        return Err(profile_limit(format!(
            "profile {} has more than {MAX_PROFILE_SPELLS} spell rows",
            profile_name(bot.class, bot.role)
        )));
    }
    spells.sort_unstable();
    spells.dedup();
    actions.extend(spells.into_iter().map(ProvisionAction::Spell));
    let tree = preferred_talent_tree(bot.class, bot.role);
    actions.push(ProvisionAction::Talent(ProvisionTalent {
        preferred_tree: tree,
        talent_id: crate::actor::select_profile_talent(ctx, bot.character_guid, tree)
            .map_err(provision_refusal)?,
    }));
    actions.extend([
        ProvisionAction::Item(gear),
        ProvisionAction::Equip(gear),
        ProvisionAction::Item(bag),
        ProvisionAction::Equip(bag),
        ProvisionAction::Equip(bag),
        ProvisionAction::Equip(bag),
        ProvisionAction::Equip(bag),
        ProvisionAction::Item(item(ProvisionItemKind::Food, FOOD, 10)),
    ]);
    if bot.class != class::WARRIOR {
        actions.push(ProvisionAction::Item(item(
            ProvisionItemKind::Drink,
            DRINK,
            10,
        )));
    }
    actions.push(ProvisionAction::Item(item(
        ProvisionItemKind::Potion,
        POTION,
        5,
    )));
    if bot.class == class::WARRIOR {
        actions.push(ProvisionAction::Item(item(
            ProvisionItemKind::Ammo,
            AMMO,
            200,
        )));
    }
    let reagent = match bot.class {
        class::PALADIN => Some(PALADIN_REAGENT),
        class::PRIEST => Some(PRIEST_REAGENT),
        class::MAGE => Some(MAGE_REAGENT),
        _ => None,
    };
    if let Some(entry) = reagent {
        actions.push(ProvisionAction::Item(item(
            ProvisionItemKind::Reagent,
            entry,
            5,
        )));
    }
    actions.extend([
        ProvisionAction::Item(item(ProvisionItemKind::Supply, BANDAGE, 5)),
        ProvisionAction::Item(item(ProvisionItemKind::Supply, HEARTHSTONE, 1)),
        ProvisionAction::Audit,
    ]);
    Ok(actions)
}

fn loose_slot(ctx: &ReducerContext, guid: u64, entry: u32) -> Option<u8> {
    ctx.db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .filter(|row| {
            ((23..=38).contains(&row.slot) || (120..=191).contains(&row.slot)) && row.entry == entry
        })
        .map(|row| row.slot)
        .min()
}

fn carried_count(ctx: &ReducerContext, guid: u64, entry: u32) -> u32 {
    ctx.db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .filter(|row| (row.slot <= 38 || (120..=191).contains(&row.slot)) && row.entry == entry)
        .map(|row| row.stack_count)
        .sum()
}

fn provision_refusal(refusal: crate::actor::ActionRefusal) -> ProvisionRefusal {
    let kind = match refusal.kind {
        crate::actor::ActionRefusalKind::MissingResource => ProvisionRefusalKind::MissingResource,
        crate::actor::ActionRefusalKind::InventoryFull => ProvisionRefusalKind::InventoryFull,
        crate::actor::ActionRefusalKind::Class => ProvisionRefusalKind::Class,
        crate::actor::ActionRefusalKind::Level => ProvisionRefusalKind::Level,
        crate::actor::ActionRefusalKind::Prerequisite => ProvisionRefusalKind::Prerequisite,
        crate::actor::ActionRefusalKind::ProfileLimit => ProvisionRefusalKind::ProfileLimit,
        _ => ProvisionRefusalKind::CoreGate,
    };
    ProvisionRefusal {
        kind,
        detail: refusal.detail,
    }
}

fn core_refusal(refusal: crate::actor::ActionRefusal) -> ProvisionOutcome {
    let refusal = provision_refusal(refusal);
    let kind = refusal.kind;
    if matches!(
        kind,
        ProvisionRefusalKind::InventoryFull | ProvisionRefusalKind::ProfileLimit
    ) {
        ProvisionOutcome::Stopped(refusal)
    } else {
        ProvisionOutcome::Refused(refusal)
    }
}

fn item_refusal(refusal: lyracore_shared::item::ItemRefusal) -> ProvisionOutcome {
    let detail = refusal.as_tag().to_string();
    let refusal = ProvisionRefusal {
        kind: match refusal {
            lyracore_shared::item::ItemRefusal::InventoryFull => {
                ProvisionRefusalKind::InventoryFull
            }
            lyracore_shared::item::ItemRefusal::ItemNotFound => {
                ProvisionRefusalKind::MissingResource
            }
            lyracore_shared::item::ItemRefusal::RequiredSkill => ProvisionRefusalKind::Prerequisite,
            _ => ProvisionRefusalKind::CoreGate,
        },
        detail,
    };
    if refusal.kind == ProvisionRefusalKind::InventoryFull {
        ProvisionOutcome::Stopped(refusal)
    } else {
        ProvisionOutcome::Refused(refusal)
    }
}

fn request_item(ctx: &ReducerContext, guid: u64, desired: ProvisionItem) -> ProvisionOutcome {
    match crate::actor::reconcile_profile_item(ctx, guid, desired.entry, desired.target_count) {
        Ok(0) => ProvisionOutcome::Satisfied,
        Ok(count) => ProvisionOutcome::Applied(count),
        Err(refusal) => core_refusal(refusal),
    }
}

fn recover(ctx: &ReducerContext, guid: u64, desired: ProvisionItem) -> Execution {
    let Some(slot) = loose_slot(ctx, guid, desired.entry) else {
        return match request_item(ctx, guid, desired) {
            ProvisionOutcome::Applied(count) => Execution {
                outcome: ProvisionOutcome::Applied(count),
                effect: ProvisionEffect::RecoveryGrant,
            },
            ProvisionOutcome::Satisfied => Execution::other(ProvisionOutcome::Deferred(
                ProvisionDeferral::MissingConsumable,
            )),
            outcome => Execution::other(outcome),
        };
    };
    match crate::actor::use_item(ctx, guid, slot) {
        Ok(()) => Execution {
            outcome: ProvisionOutcome::Applied(1),
            effect: ProvisionEffect::RecoveryUse,
        },
        Err(refusal) => Execution::other(item_refusal(refusal)),
    }
}

fn execute(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    action: ProvisionAction,
    now: i64,
) -> Execution {
    let Some(entity) = ctx.db.game_world_entity().guid().find(bot.character_guid) else {
        return Execution::other(ProvisionOutcome::Refused(ProvisionRefusal {
            kind: ProvisionRefusalKind::CoreGate,
            detail: "bot is not in world".to_string(),
        }));
    };
    match action {
        ProvisionAction::RecoverHealth => {
            if entity.health.saturating_mul(2) >= entity.max_health {
                Execution::other(ProvisionOutcome::Satisfied)
            } else {
                let desired = if entity.combat_until_ms > (now / 1000) as u64 {
                    item(ProvisionItemKind::Potion, POTION, 5)
                } else {
                    item(ProvisionItemKind::Food, FOOD, 10)
                };
                recover(ctx, bot.character_guid, desired)
            }
        }
        ProvisionAction::RecoverPower => {
            if bot.class == class::WARRIOR
                || entity.max_power == 0
                || entity.power.saturating_mul(2) >= entity.max_power
            {
                Execution::other(ProvisionOutcome::Satisfied)
            } else {
                recover(
                    ctx,
                    bot.character_guid,
                    item(ProvisionItemKind::Drink, DRINK, 10),
                )
            }
        }
        ProvisionAction::Skill(line) => Execution::other(
            match crate::actor::reconcile_profile_skill(ctx, bot.character_guid, line) {
                Ok(true) => ProvisionOutcome::Applied(1),
                Ok(false) => ProvisionOutcome::Satisfied,
                Err(refusal) => core_refusal(refusal),
            },
        ),
        ProvisionAction::Spell(spell) => Execution::other(
            match crate::actor::reconcile_profile_spell(ctx, bot.character_guid, spell) {
                Ok(true) => ProvisionOutcome::Applied(1),
                Ok(false) => ProvisionOutcome::Satisfied,
                Err(refusal) => core_refusal(refusal),
            },
        ),
        ProvisionAction::Talent(choice) => Execution::other(match choice.talent_id {
            None => ProvisionOutcome::Satisfied,
            Some(talent) => {
                match crate::actor::learn_profile_talent(ctx, bot.character_guid, talent) {
                    Ok(true) => ProvisionOutcome::Applied(1),
                    Ok(false) => ProvisionOutcome::Satisfied,
                    Err(refusal) => core_refusal(refusal),
                }
            }
        }),
        ProvisionAction::Item(desired) => {
            Execution::other(request_item(ctx, bot.character_guid, desired))
        }
        ProvisionAction::Equip(desired) => {
            let Some(slot) = loose_slot(ctx, bot.character_guid, desired.entry) else {
                return Execution::other(
                    if carried_count(ctx, bot.character_guid, desired.entry) > 0 {
                        ProvisionOutcome::Satisfied
                    } else {
                        ProvisionOutcome::Deferred(ProvisionDeferral::MissingConsumable)
                    },
                );
            };
            Execution::other(
                match crate::actor::equip_profile_upgrade(ctx, bot.character_guid, slot) {
                    Ok(true) => ProvisionOutcome::Applied(1),
                    Ok(false) => ProvisionOutcome::Satisfied,
                    Err(refusal) => item_refusal(refusal),
                },
            )
        }
        ProvisionAction::Hold(ProvisionHold::Dead) => {
            Execution::other(ProvisionOutcome::Deferred(ProvisionDeferral::Dead))
        }
        ProvisionAction::Hold(ProvisionHold::InCombat) => {
            Execution::other(ProvisionOutcome::Deferred(ProvisionDeferral::InCombat))
        }
        ProvisionAction::Audit => Execution::other(ProvisionOutcome::Satisfied),
    }
}

fn record(
    state: &mut PlayerbotsProvisioning,
    now: i64,
    action: ProvisionAction,
    outcome: ProvisionOutcome,
) {
    if state.history.len() >= HISTORY_LIMIT {
        state.history.remove(0);
    }
    state.history.push(ProvisionDecision {
        at_micros: now,
        cause: state.cause,
        profile: state.profile.clone(),
        revision: state.revision,
        action,
        outcome,
    });
}

fn finish_cycle(state: &mut PlayerbotsProvisioning, now: i64) {
    state.action_cursor = 0;
    state.cause = ProvisionCause::Periodic;
    state.next_repair_micros = now.saturating_add(REPAIR_INTERVAL_MICROS);
}

/// Perform at most one admitted profile mutation. `Worked` consumes the runner tick. A recorded
/// refusal, deferral, or stop backs off and lets the runner continue its selected action.
pub(super) fn reconcile_due(ctx: &ReducerContext, bot: &PlayerbotsBot, now: i64) -> ReconcileStep {
    if bot.controller != Controller::Cohort
        || crate::actor::sessionless_action_gate(ctx, bot.character_guid).is_err()
    {
        return ReconcileStep::Ready;
    }
    let Some(entity) = ctx.db.game_world_entity().guid().find(bot.character_guid) else {
        return ReconcileStep::Ready;
    };
    let rows = ctx.db.pkg_playerbots_provisioning();
    let mut state = rows
        .character_guid()
        .find(bot.character_guid)
        .unwrap_or_else(|| {
            let state = PlayerbotsProvisioning {
                character_guid: bot.character_guid,
                profile: profile_name(bot.class, bot.role),
                revision: PROFILE_REVISION,
                free_grants: true,
                armed_level: entity.level,
                cause: ProvisionCause::Periodic,
                action_cursor: 0,
                next_repair_micros: now,
                history: vec![],
            };
            rows.insert(state)
        });
    if now < state.next_repair_micros {
        return ReconcileStep::Ready;
    }
    if entity.dead {
        let action = ProvisionAction::Hold(ProvisionHold::Dead);
        let outcome = execute(ctx, bot, action, now).outcome;
        record(&mut state, now, action, outcome);
        state.next_repair_micros = now.saturating_add(RETRY_INTERVAL_MICROS);
        rows.character_guid().update(state);
        return ReconcileStep::Recorded;
    }
    if entity.combat_until_ms > (now / 1000) as u64
        && entity.health.saturating_mul(2) >= entity.max_health
    {
        let action = ProvisionAction::Hold(ProvisionHold::InCombat);
        let outcome = execute(ctx, bot, action, now).outcome;
        record(&mut state, now, action, outcome);
        state.next_repair_micros = now.saturating_add(RETRY_INTERVAL_MICROS);
        rows.character_guid().update(state);
        return ReconcileStep::Recorded;
    }

    let actions = match profile_actions(ctx, bot) {
        Ok(actions) => actions,
        Err(refusal) => {
            let action = ProvisionAction::Audit;
            record(&mut state, now, action, ProvisionOutcome::Stopped(refusal));
            finish_cycle(&mut state, now);
            rows.character_guid().update(state);
            return ReconcileStep::Recorded;
        }
    };
    for _ in 0..MAX_ACTION_SCAN {
        let Some(action) = actions.get(usize::from(state.action_cursor)).copied() else {
            finish_cycle(&mut state, now);
            rows.character_guid().update(state);
            return ReconcileStep::Ready;
        };
        let execution = execute(ctx, bot, action, now);
        let effect = execution.effect;
        let outcome = execution.outcome;
        if matches!(outcome, ProvisionOutcome::Satisfied) {
            state.action_cursor = state.action_cursor.saturating_add(1);
            continue;
        }

        let worked = matches!(outcome, ProvisionOutcome::Applied(_));
        let supplied_recovery = effect == ProvisionEffect::RecoveryGrant;
        let stopped = matches!(outcome, ProvisionOutcome::Stopped(_));
        record(&mut state, now, action, outcome);
        if stopped {
            finish_cycle(&mut state, now);
        } else if supplied_recovery {
            state.next_repair_micros = now.saturating_add(STEP_INTERVAL_MICROS);
        } else {
            state.action_cursor = state.action_cursor.saturating_add(1);
            state.next_repair_micros = now.saturating_add(if worked {
                STEP_INTERVAL_MICROS
            } else {
                RETRY_INTERVAL_MICROS
            });
        }
        rows.character_guid().update(state);
        return if worked {
            ReconcileStep::Worked
        } else {
            ReconcileStep::Recorded
        };
    }

    finish_cycle(&mut state, now);
    rows.character_guid().update(state);
    ReconcileStep::Ready
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_profile_names_its_free_grant_policy() {
        for class in [class::WARRIOR, class::PALADIN, class::PRIEST, class::MAGE] {
            for role in [super::super::ROLE_TANK, ROLE_HEALER, ROLE_DPS] {
                assert!(profile_name(class, role).ends_with("-free"));
            }
        }
    }
}
