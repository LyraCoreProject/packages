//! One controller, one retained objective, and one foreground action per bot.

use super::actions;
use super::decision::{
    self, Action, ActionNode, Candidate, CastAction, DecisionRefusal, MoveTarget, Readiness,
    Reason, Strategy, Trigger,
};
use super::{
    pkg_playerbots_bot, pkg_playerbots_personality, pkg_playerbots_rotation, PlayerbotsBot,
    PlayerbotsRotation,
};
use crate::{
    game_character_quest, game_creature_spline, game_gameobject, game_spell, game_world_entity,
};
use spacetimedb::{reducer, table, ReducerContext, Table};

pub const BATCH_LIMIT: usize = 16;
const INTERVAL: i64 = 1_000_000;
const OBJECTIVE_LIFETIME: i64 = 120_000_000;
const STALL_INTERVAL: i64 = 10_000_000;
const DEFER_INTERVAL: i64 = 30_000_000;
const HISTORY_LIMIT: usize = 8;
const RECOVERY_SCAN_LIMIT: usize = 24;
const FAILURE_LIMIT: usize = 4;
const CATALOG_REVISION: u64 = 1;
const DEFENSE_PRIORITY: i32 = 600;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    Legacy,
    RecordOnly,
    Cohort,
    Frozen,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct Destination {
    pub map_id: u32,
    pub instance_id: u64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub geometry_revision: Option<u64>,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveKind {
    ReturnHome,
    Companion,
    Quest,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveStage {
    Travelling,
    Completed,
    Deferred,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct Objective {
    pub identity: u64,
    pub kind: ObjectiveKind,
    pub destination: Destination,
    pub stage: ObjectiveStage,
    pub deadline_micros: i64,
    pub last_verified_progress_micros: Option<i64>,
    pub started_micros: i64,
    pub catalog_revision: u64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    NoMovement,
    DestinationUnavailable,
    Deadline,
    CastLost,
    ActionRefused(crate::actor::ActionRefusalKind),
    CastRefused(crate::spell::CastRefusalKind),
    Decision(DecisionRefusal),
    PartyFactsUnavailable,
    PartyReadUnavailable(crate::group::PartyFactsUnavailableReason),
    RoleFactsUnavailable(super::companion::RoleReadError),
    QuestTargetMissing,
    QuestReadLimit,
    QuestRespawn,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct FailureRecord {
    pub reason: Failure,
    pub at_micros: i64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct DeferredDestination {
    pub destination: Destination,
    pub until_micros: i64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct MovementRun {
    pub destination: Destination,
    pub from_x: f32,
    pub from_y: f32,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub enum Running {
    Movement(MovementRun),
    Cast(crate::spell::CastHandle),
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct Foreground {
    pub candidate: Candidate,
    pub generation: u64,
    pub map_id: u32,
    pub instance_id: u64,
    pub started_micros: i64,
    pub running: Running,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct MovementProgress {
    pub observed_micros: i64,
    pub x: f32,
    pub y: f32,
    pub arrived: bool,
}
#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct CombatProgress {
    pub observed_micros: i64,
    pub target: u64,
    pub health: u32,
}
#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct CastProgress {
    pub observed_micros: i64,
    pub scheduled_id: u64,
    pub spell: u32,
    pub target: u64,
}
#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct QuestProgress {
    pub observed_micros: i64,
    pub quest: u32,
    pub credit: u64,
    pub rewarded: bool,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub enum RunnerOutcome {
    Initial,
    Recorded,
    Waiting,
    Arrived,
    Accepted,
    CastFinished(crate::spell::CastFinish),
    Refused(Failure),
    Cancelled,
    Frozen,
    Provisioning,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct Transition {
    pub at_micros: i64,
    pub chosen: Option<Candidate>,
    pub outcome: RunnerOutcome,
}

/// Backfilled for at most BATCH_LIMIT due bots per pass. Legacy goals keep their stored meaning.
/// The public row is the explanation read; ages are relative to observed_micros.
#[table(accessor = pkg_playerbots_runner, public)]
pub struct PlayerbotsRunner {
    #[primary_key]
    pub character_guid: u64,
    pub generation: u64,
    pub objective_sequence: u64,
    pub objective: Option<Objective>,
    pub foreground: Option<Foreground>,
    pub chosen: Option<Candidate>,
    pub candidate_order: Vec<Candidate>,
    pub last_outcome: RunnerOutcome,
    pub observed_micros: i64,
    pub progress_age_micros: Option<i64>,
    pub retry_count: u8,
    pub next_eligible_micros: i64,
    pub retry_candidate: Option<decision::CandidateId>,
    pub failures: Vec<FailureRecord>,
    pub deferred_destinations: Vec<DeferredDestination>,
    pub movement_progress: Option<MovementProgress>,
    pub combat_progress: Option<CombatProgress>,
    pub cast_progress: Option<CastProgress>,
    pub quest_progress: Vec<QuestProgress>,
    pub history: Vec<Transition>,
    pub last_stall_check_micros: i64,
    pub last_target_health: Option<CombatProgress>,
    pub defense_target: Option<u64>,
    pub transitions: u32,
    pub route_expansions: u32,
    pub route_budget: u32,
    /// The stable party leader identity. A moving leader only refreshes the companion destination.
    #[default(None::<u64>)]
    pub companion_leader_guid: Option<u64>,
    /// The injured party member retained from CastingPosition movement through cast completion.
    #[default(None::<u64>)]
    pub companion_heal_target_guid: Option<u64>,
    /// The engaged enemy retained until it dies, disappears, becomes controlled, or the leader
    /// designates another engaged enemy.
    #[default(None::<u64>)]
    pub companion_fight_target_guid: Option<u64>,
    /// The party member retained while a between-fight buff repairs its casting position.
    #[default(None::<u64>)]
    pub companion_buff_target_guid: Option<u64>,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_runner(ctx, character_guid) {
    ctx.db.pkg_playerbots_runner().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_runner());

#[table(accessor = pkg_playerbots_scheduler, public)]
pub struct PlayerbotsScheduler {
    #[primary_key]
    pub id: u8,
    pub observed_micros: i64,
    pub processed: u32,
    pub processed_guids: Vec<u64>,
    pub excess_due: bool,
    pub oldest_deferred_lag_micros: i64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanStage {
    Pending,
    Complete,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryResult {
    Pending,
    Missing,
    Rotation(u64),
}

enum RecoveryLookup {
    Pending,
    Missing,
    Spell(PlayerbotsRotation),
}

/// Each pass scans at most RECOVERY_SCAN_LIMIT indexed rows and checks at most two retained rows.
/// Completed scans restart on the next pass; their result remains usable while rescanning.
/// Transfer restarts the scan against the destination's current rotations and spellbook.
#[table(accessor = pkg_playerbots_recovery_scan, public)]
pub struct PlayerbotsRecoveryScan {
    #[primary_key]
    pub character_guid: u64,
    pub class: u8,
    pub role: u8,
    pub stage: ScanStage,
    pub after_rotation_id: u64,
    pub best_rotation_id: Option<u64>,
    pub result: RecoveryResult,
    pub rows_scanned: u32,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_recovery_scan(ctx, character_guid) {
    ctx.db.pkg_playerbots_recovery_scan().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_recovery_scan());

fn recovery_spell(ctx: &ReducerContext, bot: &PlayerbotsBot) -> RecoveryLookup {
    let scans = ctx.db.pkg_playerbots_recovery_scan();
    let existing = scans.character_guid().find(bot.character_guid);
    let replacing = existing.is_some();
    let mut scan = existing
        .filter(|s| (s.class, s.role) == (bot.class, bot.role))
        .unwrap_or(PlayerbotsRecoveryScan {
            character_guid: bot.character_guid,
            class: bot.class,
            role: bot.role,
            stage: ScanStage::Pending,
            after_rotation_id: 0,
            best_rotation_id: None,
            result: RecoveryResult::Pending,
            rows_scanned: 0,
        });
    let rotations = ctx.db.pkg_playerbots_rotation();
    let valid = |row: &PlayerbotsRotation| {
        (row.class, row.role, row.condition)
            == (bot.class, bot.role, super::cond::ALLY_HP_BELOW_PCT)
            && crate::spell::knows_spell(ctx, bot.character_guid, row.spell_id)
            && ctx
                .db
                .game_spell()
                .spell_id()
                .find(row.spell_id)
                .is_some_and(|spell| spell.cast_flags & crate::spell::SPELL_ATTR_CHANNELED == 0)
    };
    if scan.stage == ScanStage::Complete {
        scan.after_rotation_id = 0;
        scan.best_rotation_id = None;
    }
    let mut best = scan
        .best_rotation_id
        .and_then(|id| rotations.id().find(id))
        .filter(&valid);
    let rows: Vec<_> = rotations
        .by_recovery_scan()
        .filter((
            bot.class,
            bot.role,
            super::cond::ALLY_HP_BELOW_PCT,
            (
                std::ops::Bound::Excluded(scan.after_rotation_id),
                std::ops::Bound::Unbounded,
            ),
        ))
        .take(RECOVERY_SCAN_LIMIT)
        .collect();
    scan.rows_scanned = rows.len() as u32;
    scan.stage = if rows.len() < RECOVERY_SCAN_LIMIT {
        ScanStage::Complete
    } else {
        ScanStage::Pending
    };
    for row in rows {
        scan.after_rotation_id = row.id;
        if valid(&row)
            && best.as_ref().is_none_or(|b| {
                (row.priority, row.spell_id, row.id) < (b.priority, b.spell_id, b.id)
            })
        {
            best = Some(row);
        }
    }
    scan.best_rotation_id = best.as_ref().map(|r| r.id);
    if scan.stage == ScanStage::Complete {
        scan.result = scan
            .best_rotation_id
            .map_or(RecoveryResult::Missing, RecoveryResult::Rotation);
    }
    let lookup = match scan.result {
        RecoveryResult::Pending => RecoveryLookup::Pending,
        RecoveryResult::Missing => RecoveryLookup::Missing,
        RecoveryResult::Rotation(id) => match rotations.id().find(id).filter(valid) {
            Some(row) => RecoveryLookup::Spell(row),
            None => {
                scan.result = RecoveryResult::Missing;
                RecoveryLookup::Missing
            }
        },
    };
    if replacing {
        scans.character_guid().update(scan);
    } else {
        scans.insert(scan);
    }
    lookup
}

impl PlayerbotsRunner {
    fn initial(guid: u64, now: i64) -> Self {
        Self {
            character_guid: guid,
            generation: 0,
            objective_sequence: 0,
            objective: None,
            foreground: None,
            chosen: None,
            candidate_order: vec![],
            last_outcome: RunnerOutcome::Initial,
            observed_micros: now,
            progress_age_micros: None,
            retry_count: 0,
            next_eligible_micros: now,
            retry_candidate: None,
            failures: vec![],
            deferred_destinations: vec![],
            movement_progress: None,
            combat_progress: None,
            cast_progress: None,
            quest_progress: vec![],
            history: vec![],
            last_stall_check_micros: now,
            last_target_health: None,
            defense_target: None,
            transitions: 0,
            route_expansions: 0,
            route_budget: 0,
            companion_leader_guid: None,
            companion_heal_target_guid: None,
            companion_fight_target_guid: None,
            companion_buff_target_guid: None,
        }
    }

    fn failure(&mut self, reason: Failure, now: i64) {
        self.retry_count = self.retry_count.saturating_add(1).min(3);
        bounded_push(
            &mut self.failures,
            FailureRecord {
                reason,
                at_micros: now,
            },
            FAILURE_LIMIT,
        );
        self.last_outcome = RunnerOutcome::Refused(reason);
    }

    fn save(mut self, ctx: &ReducerContext) {
        self.observed_micros = ctx.timestamp.to_micros_since_unix_epoch();
        self.next_eligible_micros = self
            .next_eligible_micros
            .max(self.observed_micros.saturating_add(INTERVAL));
        self.progress_age_micros = self.objective.as_ref().map(|o| {
            self.observed_micros
                .saturating_sub(o.last_verified_progress_micros.unwrap_or(o.started_micros))
        });
        let changed = self.history.last().is_none_or(|t| {
            t.chosen != self.chosen
                || std::mem::discriminant(&t.outcome) != std::mem::discriminant(&self.last_outcome)
        });
        if changed {
            let transition = Transition {
                at_micros: self.observed_micros,
                chosen: self.chosen,
                outcome: self.last_outcome.clone(),
            };
            bounded_push(&mut self.history, transition, HISTORY_LIMIT);
        }
        let rows = ctx.db.pkg_playerbots_runner();
        if rows.character_guid().find(self.character_guid).is_some() {
            rows.character_guid().update(self);
        } else {
            rows.insert(self);
        }
    }
}

fn bounded_push<T>(values: &mut Vec<T>, value: T, limit: usize) {
    if values.len() >= limit {
        values.remove(0);
    }
    values.push(value);
}

pub(super) fn legacy_controls(ctx: &ReducerContext, guid: u64) -> bool {
    ctx.db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .is_some_and(|bot| bot.controller == Controller::Legacy)
        && crate::actor::sessionless_action_gate(ctx, guid).is_ok()
}

pub(super) fn pass(ctx: &ReducerContext) {
    super::ensure_defaults(ctx);
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let bots = ctx.db.pkg_playerbots_bot();
    let due: Vec<_> = bots.by_due().filter(..=now).take(BATCH_LIMIT).collect();
    let guids = due.iter().map(|b| b.character_guid).collect();
    let count = due.len();
    for mut bot in due {
        let mut state = ctx
            .db
            .pkg_playerbots_runner()
            .character_guid()
            .find(bot.character_guid)
            .unwrap_or_else(|| PlayerbotsRunner::initial(bot.character_guid, now));
        if crate::actor::sessionless_action_gate(ctx, bot.character_guid).is_err() {
            if matches!(bot.controller, Controller::Legacy | Controller::Cohort) {
                stop(ctx, bot.character_guid, &mut state);
            }
            state.chosen = Some(Candidate {
                id: decision::CandidateId {
                    action: Action::Hold,
                    reason: Reason::Restricted,
                    objective: state.objective_sequence,
                },
                priority: 1000,
            });
            state.last_outcome = RunnerOutcome::Waiting;
            state.save(ctx);
        } else {
            match bot.controller {
                Controller::Legacy => {
                    state.save(ctx);
                    super::goals::think(ctx, &bot, now);
                }
                Controller::RecordOnly | Controller::Cohort => run(ctx, &bot, state, now),
                Controller::Frozen => {
                    state.save(ctx);
                }
            }
        }
        bot.scheduler_lag_micros = now.saturating_sub(bot.next_think_micros).max(0);
        bot.next_think_micros = now.saturating_add(INTERVAL);
        bots.id().update(bot);
    }
    let deferred = bots.by_due().filter(..=now).next();
    let row = PlayerbotsScheduler {
        id: 0,
        observed_micros: now,
        processed: count as u32,
        processed_guids: guids,
        excess_due: deferred.is_some(),
        oldest_deferred_lag_micros: deferred
            .map_or(0, |bot| now.saturating_sub(bot.next_think_micros).max(0)),
    };
    let rows = ctx.db.pkg_playerbots_scheduler();
    if rows.id().find(0).is_some() {
        rows.id().update(row);
    } else {
        rows.insert(row);
    }
}

/// Selection is idempotent. A changed controller cancels work before the new generation can act.
#[reducer]
pub fn playerbots_select_controller(
    ctx: &ReducerContext,
    guid: u64,
    controller: Controller,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    crate::actor::set_sessionless_action_consent(
        ctx,
        guid,
        matches!(controller, Controller::Legacy | Controller::Cohort),
    );
    if bot.controller == controller {
        return Ok(());
    }
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let mut state = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(guid)
        .unwrap_or_else(|| PlayerbotsRunner::initial(guid, now));
    stop(ctx, guid, &mut state);
    state.last_stall_check_micros = now;
    state.generation = state
        .generation
        .checked_add(1)
        .ok_or("controller generation exhausted")?;
    state.next_eligible_micros = if controller == Controller::Frozen {
        i64::MAX
    } else {
        now
    };
    state.last_outcome = if controller == Controller::Frozen {
        RunnerOutcome::Frozen
    } else {
        RunnerOutcome::Cancelled
    };
    state.save(ctx);
    bot.controller = controller;
    bot.next_think_micros = if controller == Controller::Frozen {
        i64::MAX
    } else {
        now
    };
    ctx.db.pkg_playerbots_bot().id().update(bot);
    Ok(())
}

fn stop(ctx: &ReducerContext, guid: u64, state: &mut PlayerbotsRunner) {
    if let Some(foreground) = state.foreground.take() {
        if let Running::Cast(handle) = &foreground.running {
            if state.companion_heal_target_guid == Some(handle.target_guid) {
                state.companion_heal_target_guid = None;
            }
            if state.companion_buff_target_guid == Some(handle.target_guid) {
                state.companion_buff_target_guid = None;
            }
        }
        bounded_push(
            &mut state.history,
            Transition {
                at_micros: ctx.timestamp.to_micros_since_unix_epoch(),
                chosen: Some(foreground.candidate),
                outcome: RunnerOutcome::Cancelled,
            },
            HISTORY_LIMIT,
        );
    }
    if let Some(cast) = crate::spell::pending_cast(ctx, guid) {
        if actions::observation(ctx, guid, actions::ActionKind::Cast)
            .is_some_and(|owned| owned.cast_id == cast.scheduled_id)
        {
            crate::spell::cancel_cast_attempt(ctx, guid, cast.scheduled_id);
        }
    }
    if crate::actor::sessionless_action_gate(ctx, guid).is_ok() {
        let _ = crate::actor::stop_attack(ctx, guid);
    }
    stop_movement(ctx, guid);
}

fn stop_movement(ctx: &ReducerContext, guid: u64) {
    let Some(spline) = ctx.db.game_creature_spline().guid().find(guid) else {
        return;
    };
    if !actions::observation(ctx, guid, actions::ActionKind::Move)
        .is_some_and(|owned| owned.observed_micros as u64 == spline.start_micros)
    {
        return;
    }
    if let Some(me) = ctx.db.game_world_entity().guid().find(guid) {
        let point = (me.x, me.y, me.z);
        crate::creatures::tick::emit_move_spline(
            ctx,
            guid,
            point,
            point,
            0,
            false,
            (ctx.timestamp.to_micros_since_unix_epoch() / 1000) as u32,
            me.map_id,
            me.instance_id,
            (me.grid_x, me.grid_y),
        );
    } else {
        ctx.db.game_creature_spline().guid().delete(guid);
    }
}

fn geometry_revision(ctx: &ReducerContext, map: u32) -> Option<u64> {
    crate::nav::coverage_generation(ctx, map)
}

fn distance(me: &crate::WorldEntity, dest: &Destination) -> f32 {
    ((me.x - dest.x).powi(2) + (me.y - dest.y).powi(2) + (me.z - dest.z).powi(2)).sqrt()
}

fn objective(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    party: Option<&super::companion::Party>,
    state: &mut PlayerbotsRunner,
    now: i64,
) {
    state.deferred_destinations.retain(|d| d.until_micros > now);
    let admission = if party.is_none() {
        super::quest_catalog::reconcile_active(ctx, bot.character_guid)
    } else {
        None
    };
    if let Some(admission) = admission {
        let destination = Destination {
            map_id: admission.destination.map_id,
            instance_id: admission.destination.instance_id,
            x: admission.destination.x,
            y: admission.destination.y,
            z: admission.destination.z,
            geometry_revision: None,
        };
        let changed = !super::quest_catalog::retained_matches(ctx, bot.character_guid, &admission)
            || state.objective.as_ref().is_none_or(|objective| {
                objective.kind != ObjectiveKind::Quest
                    || objective.destination != destination
                    || objective.catalog_revision != super::quest_catalog::CATALOG_REVISION
            });
        if changed {
            state.objective_sequence = state.objective_sequence.saturating_add(1);
            let identity = state.objective_sequence;
            state.objective = Some(Objective {
                identity,
                kind: ObjectiveKind::Quest,
                destination,
                stage: ObjectiveStage::Travelling,
                deadline_micros: now.saturating_add(OBJECTIVE_LIFETIME),
                last_verified_progress_micros: None,
                started_micros: now,
                catalog_revision: super::quest_catalog::CATALOG_REVISION,
            });
            super::quest_catalog::retain(ctx, bot.character_guid, identity, admission);
            state.companion_leader_guid = None;
            state.companion_heal_target_guid = None;
            state.companion_fight_target_guid = None;
            state.companion_buff_target_guid = None;
            state.retry_count = 0;
            state.last_stall_check_micros = now;
        }
    } else {
        if state
            .objective
            .as_ref()
            .is_some_and(|objective| objective.kind == ObjectiveKind::Quest)
        {
            super::quest_catalog::clear_retained(ctx, bot.character_guid);
        }
        let (kind, leader_guid, mut destination) = if let Some(party) = party {
            let destination = party.destination().or_else(|| {
                state
                    .objective
                    .as_ref()
                    .filter(|objective| {
                        objective.kind == ObjectiveKind::Companion
                            && state.companion_leader_guid == Some(party.leader_guid)
                    })
                    .map(|objective| objective.destination.clone())
            });
            (
                ObjectiveKind::Companion,
                Some(party.leader_guid),
                destination.unwrap_or(Destination {
                    map_id: me.map_id,
                    instance_id: me.instance_id,
                    x: me.x,
                    y: me.y,
                    z: me.z,
                    geometry_revision: geometry_revision(ctx, me.map_id),
                }),
            )
        } else {
            (
                ObjectiveKind::ReturnHome,
                None,
                Destination {
                    map_id: bot.home_map,
                    instance_id: 0,
                    x: bot.home_x,
                    y: bot.home_y,
                    z: bot.home_z,
                    geometry_revision: geometry_revision(ctx, bot.home_map),
                },
            )
        };
        destination.geometry_revision = geometry_revision(ctx, destination.map_id);
        let same_identity = state.objective.as_ref().is_some_and(|objective| {
            objective.kind == kind
                && (kind != ObjectiveKind::Companion || state.companion_leader_guid == leader_guid)
        });
        let replace = !same_identity
            || (kind == ObjectiveKind::ReturnHome
                && state
                    .objective
                    .as_ref()
                    .is_none_or(|objective| objective.destination != destination));
        if replace {
            state.objective_sequence = state.objective_sequence.saturating_add(1);
            state.objective = Some(Objective {
                identity: state.objective_sequence,
                kind,
                destination,
                stage: ObjectiveStage::Travelling,
                deadline_micros: if kind == ObjectiveKind::Companion {
                    i64::MAX
                } else {
                    now.saturating_add(OBJECTIVE_LIFETIME)
                },
                last_verified_progress_micros: None,
                started_micros: now,
                catalog_revision: CATALOG_REVISION,
            });
            state.companion_leader_guid = leader_guid;
            state.companion_heal_target_guid = None;
            state.companion_fight_target_guid = None;
            state.companion_buff_target_guid = None;
            state.retry_count = 0;
            state.last_stall_check_micros = now;
        } else if kind == ObjectiveKind::Companion {
            if let Some(objective) = &mut state.objective {
                objective.destination = destination;
            }
        }
    }
    if let Some(o) = &mut state.objective {
        if o.kind != ObjectiveKind::Companion
            && o.stage == ObjectiveStage::Deferred
            && !state
                .deferred_destinations
                .iter()
                .any(|d| d.destination == o.destination)
        {
            o.stage = ObjectiveStage::Travelling;
            o.deadline_micros = now.saturating_add(OBJECTIVE_LIFETIME);
            state.retry_count = 0;
            state.last_stall_check_micros = now;
        }
        if o.kind != ObjectiveKind::Companion && o.stage == ObjectiveStage::Deferred {
            if let Some(deferred) = state
                .deferred_destinations
                .iter()
                .find(|d| d.destination == o.destination)
            {
                state.next_eligible_micros = state.next_eligible_micros.max(deferred.until_micros);
            }
        }
    }
}

fn defer(state: &mut PlayerbotsRunner, now: i64) {
    if let Some(o) = &mut state.objective {
        o.stage = ObjectiveStage::Deferred;
        state
            .deferred_destinations
            .retain(|d| d.destination != o.destination);
        bounded_push(
            &mut state.deferred_destinations,
            DeferredDestination {
                destination: o.destination.clone(),
                until_micros: now.saturating_add(DEFER_INTERVAL),
            },
            FAILURE_LIMIT,
        );
    }
    state.next_eligible_micros = now.saturating_add(DEFER_INTERVAL);
}

fn observe(ctx: &ReducerContext, me: &crate::WorldEntity, state: &mut PlayerbotsRunner, now: i64) {
    if let Some(fg) = &state.foreground {
        if fg.generation != state.generation
            || (fg.map_id, fg.instance_id) != (me.map_id, me.instance_id)
        {
            stop(ctx, me.guid, state);
            state.last_outcome = RunnerOutcome::Cancelled;
        }
    }
    if let Some(fg) = &mut state.foreground {
        match &mut fg.running {
            Running::Movement(MovementRun {
                destination,
                from_x,
                from_y,
            }) => {
                let advanced = (me.x - *from_x).powi(2) + (me.y - *from_y).powi(2) > 0.05 * 0.05;
                let arrived = distance(me, destination) <= 2.05;
                if advanced || arrived {
                    state.last_stall_check_micros = now;
                    state.retry_count = 0;
                    state.movement_progress = Some(MovementProgress {
                        observed_micros: now,
                        x: me.x,
                        y: me.y,
                        arrived,
                    });
                    if let Some(o) = &mut state.objective {
                        if o.destination == *destination {
                            o.last_verified_progress_micros = Some(now);
                            state.retry_count = 0;
                            state.last_stall_check_micros = now;
                            if arrived {
                                o.stage = ObjectiveStage::Completed;
                                state.last_outcome = RunnerOutcome::Arrived;
                            }
                        }
                    }
                }
                *from_x = me.x;
                *from_y = me.y;
                if arrived {
                    state.foreground = None;
                }
            }
            Running::Cast(handle) => {
                if let Some(current) = crate::spell::pending_cast(ctx, me.guid)
                    .filter(|h| h.scheduled_id == handle.scheduled_id)
                {
                    // Direct damage moves due_micros without changing the core cast identity.
                    *handle = current;
                } else {
                    state.foreground = None;
                    state.failure(Failure::CastLost, now);
                }
            }
        }
    }
    if let Some(previous) = &mut state.last_target_health {
        if let Some(target) = ctx.db.game_world_entity().guid().find(previous.target) {
            if target.health < previous.health {
                state.combat_progress = Some(CombatProgress {
                    observed_micros: now,
                    target: target.guid,
                    health: target.health,
                });
            }
            previous.health = target.health;
        }
    }
    for quest in ctx
        .db
        .game_character_quest()
        .by_character()
        .filter(me.guid)
        .take(20)
    {
        let credit = quest.counts.iter().map(|&c| u64::from(c)).sum();
        if let Some(previous) = state
            .quest_progress
            .iter_mut()
            .find(|q| q.quest == quest.quest_entry)
        {
            if credit > previous.credit || (quest.rewarded && !previous.rewarded) {
                previous.observed_micros = now;
            }
            previous.credit = credit;
            previous.rewarded = quest.rewarded;
        } else {
            bounded_push(
                &mut state.quest_progress,
                QuestProgress {
                    observed_micros: 0,
                    quest: quest.quest_entry,
                    credit,
                    rewarded: quest.rewarded,
                },
                20,
            );
        }
    }
}

fn run(ctx: &ReducerContext, bot: &PlayerbotsBot, mut state: PlayerbotsRunner, now: i64) {
    let Some(me) = ctx.db.game_world_entity().guid().find(bot.character_guid) else {
        state.chosen = Some(Candidate {
            id: decision::CandidateId {
                action: Action::Hold,
                reason: Reason::Restricted,
                objective: state.objective_sequence,
            },
            priority: 1000,
        });
        state.last_outcome = RunnerOutcome::Waiting;
        state.save(ctx);
        return;
    };
    let party = match super::companion::human_led_party(ctx, me.guid) {
        Ok(party) => party,
        Err(unavailable) => {
            spacetimedb::log::error!(
                "party facts unavailable for member {} in group {}: {:?}",
                me.guid,
                unavailable.group_id,
                unavailable.reason
            );
            if bot.controller == Controller::Cohort {
                stop(ctx, me.guid, &mut state);
            }
            state.chosen = Some(Candidate {
                id: decision::CandidateId {
                    action: Action::Hold,
                    reason: Reason::PartyUnavailable,
                    objective: state.objective_sequence,
                },
                priority: 1000,
            });
            state.failure(Failure::PartyReadUnavailable(unavailable.reason), now);
            state.save(ctx);
            return;
        }
    };
    let prior_objective = state.objective_sequence;
    objective(ctx, bot, &me, party.as_ref(), &mut state, now);
    if bot.controller == Controller::Cohort {
        if prior_objective != state.objective_sequence && state.foreground.is_some() {
            stop(ctx, me.guid, &mut state);
        }
        observe(ctx, &me, &mut state, now);
    }
    let Some(destination) = state.objective.as_ref().map(|o| o.destination.clone()) else {
        return;
    };
    let quest_objective = state
        .objective
        .as_ref()
        .is_some_and(|objective| objective.kind == ObjectiveKind::Quest);
    let mut retained_quest = quest_objective
        .then(|| super::quest_catalog::retained(ctx, me.guid))
        .flatten();
    if bot.controller == Controller::Cohort {
        if let Some(retained) = retained_quest.as_mut() {
            super::quest_loop::invalidate_safe_position(ctx, &me, retained);
        }
    }
    let quest_plan = retained_quest
        .as_ref()
        .map(|retained| super::quest_loop::plan(ctx, &me, retained));
    if bot.controller == Controller::Cohort
        && matches!(
            quest_plan,
            Some(super::quest_loop::QuestPlan::Attack { .. })
        )
        && !me.dead
        && me.combat_until_ms <= (now.max(0) as u64 / 1000)
        && state.defense_target.is_none()
        && state
            .foreground
            .as_ref()
            .is_none_or(|foreground| foreground.candidate.id.reason != Reason::Survival)
    {
        if let Some(retained) = retained_quest.as_mut() {
            if let Some(safe) = super::quest_loop::observe_safe_position(ctx, &me, retained) {
                retained.safe_position = Some(safe);
            }
        }
    }
    let quest_wait = super::quest_catalog::active_wait_until(ctx, me.guid);
    let partition_ok = (destination.map_id, destination.instance_id) == (me.map_id, me.instance_id);
    let stop_distance = if party.is_some() { 3.05 } else { 2.05 };
    let at_destination = partition_ok && distance(&me, &destination) <= stop_distance;
    if bot.controller == Controller::Cohort {
        if let Some(o) = &mut state.objective {
            if at_destination {
                o.stage = ObjectiveStage::Completed;
            } else if o.stage == ObjectiveStage::Completed {
                o.stage = ObjectiveStage::Travelling;
                if o.kind != ObjectiveKind::Companion {
                    o.deadline_micros = now.saturating_add(OBJECTIVE_LIFETIME);
                }
                state.last_stall_check_micros = now;
            }
        }
    }
    let personality = ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(me.guid)
        .next();
    let flee_at = personality.as_ref().map_or(15, |p| p.flee_at_pct);
    let low_health = super::goals::should_flee(me.health, me.max_health, flee_at);
    let threat = state
        .defense_target
        .and_then(|guid| ctx.db.game_world_entity().guid().find(guid))
        .filter(|t| !t.dead && (t.map_id, t.instance_id) == (me.map_id, me.instance_id));
    state.defense_target = threat.as_ref().map(|target| target.guid);
    let recovery_lookup = recovery_spell(ctx, bot);
    let spell = match &recovery_lookup {
        RecoveryLookup::Spell(row) => Some(row),
        RecoveryLookup::Pending | RecoveryLookup::Missing => None,
    };
    let restricted = crate::helpers::live_entity(ctx, me.guid).is_err();
    let facts = decision::Facts {
        now,
        restricted,
        low_health,
        wounded: u64::from(me.health) * 100 < u64::from(me.max_health) * 50,
        attacked: threat.is_some(),
        away: !at_destination,
    };
    let objective_sequence = state.objective_sequence;
    let node = |action, reason, priority| {
        let mut node = ActionNode::ready(action, reason, priority);
        node.candidate.id.objective = objective_sequence;
        node
    };
    let mut travel_action = node(Action::Move(MoveTarget::Home), Reason::ReturnHome, 100);
    travel_action.readiness = if !partition_ok {
        Readiness::Refused
    } else if state.next_eligible_micros > now {
        Readiness::NotBefore(state.next_eligible_micros)
    } else {
        Readiness::Ready
    };
    travel_action
        .alternatives
        .push(node(Action::Hold, Reason::ReturnHome, 100));
    let mut recovery = node(
        spell.as_ref().map_or(Action::Hold, |spell| {
            Action::Cast(CastAction {
                target: me.guid,
                spell: spell.spell_id,
            })
        }),
        Reason::Recovery,
        800,
    );
    if matches!(recovery_lookup, RecoveryLookup::Missing) {
        recovery.readiness = Readiness::Refused;
    }
    let mut defense = node(
        threat
            .as_ref()
            .map_or(Action::Hold, |target| Action::Attack(target.guid)),
        Reason::Defense,
        DEFENSE_PRIORITY,
    );
    if threat.is_none() {
        defense.readiness = Readiness::Refused;
    }
    if let Some(target) = &threat {
        let mut close = node(
            Action::Move(MoveTarget::Entity(target.guid)),
            Reason::Defense,
            DEFENSE_PRIORITY,
        );
        if ((target.x - me.x).powi(2) + (target.y - me.y).powi(2)).sqrt() <= 4.0 {
            close.readiness = Readiness::Complete;
        }
        defense.prerequisites.push(close);
    }
    if party.is_none() {
        defense.continuers.push(travel_action.clone());
    }
    let safe_destination = retained_quest.as_ref().and_then(|retained| {
        super::quest_loop::safe_position(&me, retained).map(|safe| Destination {
            map_id: safe.map_id,
            instance_id: safe.instance_id,
            x: safe.x,
            y: safe.y,
            z: safe.z,
            geometry_revision: geometry_revision(ctx, safe.map_id),
        })
    });
    let at_safe_destination = safe_destination.as_ref().is_none_or(|safe| {
        (safe.map_id, safe.instance_id) == (me.map_id, me.instance_id)
            && distance(&me, safe) <= 2.05
    });
    let mut survival = if partition_ok
        && ((!quest_objective && !at_destination) || (quest_objective && !at_safe_destination))
    {
        node(Action::Move(MoveTarget::Home), Reason::Survival, 900)
    } else {
        node(Action::Hold, Reason::Survival, 900)
    };
    if (!quest_objective && at_destination) || (quest_objective && at_safe_destination) {
        survival.readiness = Readiness::Complete;
    }
    if let Some(deferred) = state
        .deferred_destinations
        .iter()
        .find(|d| d.destination == destination)
    {
        survival.readiness = Readiness::NotBefore(deferred.until_micros);
        survival
            .alternatives
            .push(node(Action::Hold, Reason::Survival, 900));
    }
    let retry_candidate = state.retry_candidate;
    let next_eligible_micros = state.next_eligible_micros;
    let strategy = |trigger, mut n: ActionNode| {
        if retry_candidate == Some(n.candidate.id) && next_eligible_micros > now {
            n.readiness = Readiness::NotBefore(next_eligible_micros);
        }
        Strategy {
            trigger,
            candidates: vec![n],
            defaults: vec![],
            priority_adjustment: 0,
        }
    };
    let mut strategies = vec![strategy(
        Trigger::Restricted,
        node(Action::Hold, Reason::Restricted, 1000),
    )];
    let mut companion_heal_target = state.companion_heal_target_guid;
    let previous_fight_target = state.companion_fight_target_guid;
    let mut companion_fight_target = previous_fight_target;
    let mut companion_buff_target = state.companion_buff_target_guid;
    if me.dead {
        strategies.push(strategy(
            Trigger::Always,
            node(Action::Resurrect, Reason::Resurrection, 1000),
        ));
    } else {
        strategies.push(strategy(Trigger::LowHealth, survival));
        if party.is_none() || bot.role != super::ROLE_HEALER {
            strategies.push(strategy(Trigger::Wounded, recovery));
        }
        strategies.push(strategy(Trigger::Attacked, defense));
        if let Some(party) = party.as_ref() {
            let selection = super::companion::strategy(
                ctx,
                bot,
                &me,
                party,
                !low_health || at_destination,
                state.objective_sequence,
                state.companion_heal_target_guid,
                state.companion_fight_target_guid,
                state.companion_buff_target_guid,
            );
            match selection {
                Ok(selection) => {
                    if let Some(unavailable) = selection.read_failure {
                        spacetimedb::log::error!(
                            "role facts unavailable for member {}: {:?}",
                            me.guid,
                            unavailable
                        );
                        state.failure(Failure::RoleFactsUnavailable(unavailable), now);
                    }
                    companion_heal_target = selection.heal_target;
                    companion_fight_target = selection.fight_target;
                    companion_buff_target = selection.buff_target;
                    strategies.push(selection.strategy);
                }
                Err(unavailable) => {
                    spacetimedb::log::error!(
                        "role facts unavailable for member {}: {:?}",
                        me.guid,
                        unavailable
                    );
                    companion_heal_target = None;
                    companion_fight_target = None;
                    companion_buff_target = None;
                    state.failure(Failure::RoleFactsUnavailable(unavailable), now);
                    strategies.push(strategy(
                        Trigger::Always,
                        node(Action::Hold, Reason::RoleUnavailable, 750),
                    ));
                }
            }
        } else {
            companion_heal_target = None;
            companion_fight_target = None;
            companion_buff_target = None;
            if let Some(plan) = quest_plan {
                let unavailable = node(Action::Hold, Reason::Quest, 110);
                if state.retry_candidate == Some(unavailable.candidate.id)
                    && state.next_eligible_micros > now
                {
                    strategies.push(strategy(Trigger::Always, unavailable));
                } else {
                    match super::quest_loop::strategy(
                        ctx,
                        bot,
                        &me,
                        plan,
                        state.objective_sequence,
                        !at_destination,
                        travel_action.clone(),
                    ) {
                        Ok(quest_strategy) => {
                            if state.retry_candidate == Some(unavailable.candidate.id) {
                                state.retry_candidate = None;
                                state.retry_count = 0;
                            }
                            strategies.push(strategy(Trigger::Always, quest_strategy));
                        }
                        Err(read_error) => {
                            spacetimedb::log::error!(
                                "quest combat rotation unavailable for {}: {:?}",
                                me.guid,
                                read_error
                            );
                            state.failure(Failure::QuestReadLimit, now);
                            state.retry_candidate = Some(unavailable.candidate.id);
                            state.next_eligible_micros =
                                now.saturating_add(if state.retry_count >= 3 {
                                    DEFER_INTERVAL
                                } else {
                                    INTERVAL * i64::from(state.retry_count)
                                });
                            strategies.push(strategy(Trigger::Always, unavailable));
                        }
                    }
                }
            } else if quest_objective || quest_wait.is_some_and(|until| until > now) {
                strategies.push(strategy(
                    Trigger::Always,
                    node(Action::Hold, Reason::Quest, 110),
                ));
            } else {
                strategies.push(strategy(Trigger::Away, travel_action));
            }
        }
    }
    strategies.push(strategy(
        Trigger::Always,
        node(Action::Hold, Reason::Idle, 0),
    ));
    state.companion_heal_target_guid = companion_heal_target;
    state.companion_fight_target_guid = companion_fight_target;
    state.companion_buff_target_guid = companion_buff_target;
    let decision = decision::choose(&facts, &strategies, decision::LIMITS);
    state.candidate_order = decision.order;
    state.transitions = decision.transitions as u32;
    state.route_expansions = 0;
    state.route_budget = decision.route_expansions;
    let chosen = decision.chosen;
    if bot.controller == Controller::RecordOnly {
        state.chosen = chosen;
        state.last_outcome = RunnerOutcome::Recorded;
        state.save(ctx);
        return;
    }
    let party_holds_control = party
        .as_ref()
        .is_some_and(|party| !party.enemies.is_empty() && companion_fight_target.is_none());
    if party_holds_control
        || (previous_fight_target.is_some() && previous_fight_target != companion_fight_target)
    {
        let _ = crate::actor::stop_attack(ctx, me.guid);
    }
    if let Some(deadline) = state
        .objective
        .as_ref()
        .filter(|o| {
            o.kind != ObjectiveKind::Companion
                && o.stage == ObjectiveStage::Travelling
                && now >= o.deadline_micros
        })
        .map(|o| o.deadline_micros)
    {
        if let Some(foreground) = state.foreground.clone() {
            if let Running::Cast(handle) = foreground.running {
                if crate::spell::expire_cast_attempt(ctx, me.guid, handle.scheduled_id, deadline) {
                    state.foreground = None;
                    bounded_push(
                        &mut state.history,
                        Transition {
                            at_micros: now,
                            chosen: Some(foreground.candidate),
                            outcome: RunnerOutcome::CastFinished(crate::spell::CastFinish::Expired),
                        },
                        HISTORY_LIMIT,
                    );
                }
            }
        }
        stop(ctx, me.guid, &mut state);
        state.failure(Failure::Deadline, now);
        defer(&mut state, now);
        state.save(ctx);
        return;
    }
    if let Some(fg) = &state.foreground {
        let incompatible = chosen.is_some_and(|c| c.id != fg.candidate.id);
        let casting_target_invalid = match fg.candidate.id {
            decision::CandidateId {
                action: Action::Move(MoveTarget::CastingPosition(target)),
                reason: Reason::CastingPosition,
                ..
            } => {
                state.companion_heal_target_guid != Some(target)
                    && state.companion_fight_target_guid != Some(target)
                    && state.companion_buff_target_guid != Some(target)
            }
            decision::CandidateId {
                action: Action::Move(MoveTarget::CastingPosition(target)),
                reason: Reason::FightPosition,
                ..
            } => state.companion_fight_target_guid != Some(target),
            decision::CandidateId {
                action: Action::Move(MoveTarget::Entity(target)),
                reason: Reason::MeleePosition,
                ..
            } => state.companion_fight_target_guid != Some(target),
            decision::CandidateId {
                action: Action::Move(MoveTarget::CastingPosition(target)),
                reason: Reason::MeleePosition,
                ..
            } => state.companion_fight_target_guid != Some(target),
            decision::CandidateId {
                action: Action::Move(MoveTarget::CastingPosition(target)),
                reason: Reason::BuffPosition,
                ..
            } => state.companion_buff_target_guid != Some(target),
            decision::CandidateId {
                action: Action::Cast(CastAction { target, .. }),
                reason: Reason::Heal,
                ..
            } => state.companion_heal_target_guid != Some(target),
            decision::CandidateId {
                action: Action::Cast(CastAction { target, .. }),
                reason: Reason::TankFight | Reason::DamageFight,
                ..
            } => state.companion_fight_target_guid != Some(target),
            decision::CandidateId {
                action: Action::Cast(CastAction { target, .. }),
                reason: Reason::Buff,
                ..
            } => state.companion_buff_target_guid != Some(target),
            _ => false,
        };
        let preempts =
            casting_target_invalid || chosen.is_some_and(|c| c.priority > fg.candidate.priority);
        if incompatible && preempts {
            stop(ctx, me.guid, &mut state);
            state.last_outcome = RunnerOutcome::Cancelled;
        } else if matches!(fg.running, Running::Cast(_)) {
            state.chosen = Some(fg.candidate);
            state.last_outcome = RunnerOutcome::Waiting;
            state.save(ctx);
            return;
        } else if incompatible && !preempts {
            // A completed leg releases ownership; a running leg retains it until observed arrival.
            if ctx
                .db
                .game_creature_spline()
                .guid()
                .find(me.guid)
                .is_some_and(|s| {
                    s.start_micros.saturating_add(u64::from(s.dur_ms) * 1000) > now.max(0) as u64
                })
            {
                state.chosen = Some(fg.candidate);
                state.last_outcome = RunnerOutcome::Waiting;
                state.save(ctx);
                return;
            }
            state.foreground = None;
        }
    }
    if bot.controller == Controller::Cohort
        && state.foreground.is_none()
        && chosen.is_none_or(|candidate| candidate.priority < DEFENSE_PRIORITY)
    {
        match super::provisioning::reconcile_due(ctx, bot, now) {
            super::provisioning::ReconcileStep::Ready
            | super::provisioning::ReconcileStep::Recorded => {}
            super::provisioning::ReconcileStep::Worked => {
                let candidate = Candidate {
                    id: decision::CandidateId {
                        action: Action::Hold,
                        reason: Reason::Provisioning,
                        objective: state.objective_sequence,
                    },
                    priority: 0,
                };
                state.chosen = Some(candidate);
                state.candidate_order = vec![candidate];
                state.last_outcome = RunnerOutcome::Provisioning;
                state.save(ctx);
                return;
            }
        }
    }
    state.chosen = chosen;
    if let (Some(reason), Some(candidate)) =
        (quest_plan.and_then(super::quest_loop::wait_reason), chosen)
    {
        if candidate.id.reason == Reason::Quest && candidate.id.action == Action::Hold {
            state.failure(
                match reason {
                    super::quest_loop::WaitReason::MissingTarget => Failure::QuestTargetMissing,
                    super::quest_loop::WaitReason::ReadLimit => Failure::QuestReadLimit,
                    super::quest_loop::WaitReason::Respawn => Failure::QuestRespawn,
                },
                now,
            );
            state.next_eligible_micros = now.saturating_add(DEFER_INTERVAL);
            state.next_eligible_micros =
                state.next_eligible_micros.max(now.saturating_add(INTERVAL));
            state.save(ctx);
            return;
        }
    }
    if let Some(candidate) = chosen {
        execute(ctx, &me, &destination, candidate, &mut state, now);
    } else if let Some(reason) = decision.refusals.first() {
        state.failure(Failure::Decision(*reason), now);
    }
    state.next_eligible_micros = state.next_eligible_micros.max(now.saturating_add(INTERVAL));
    state.save(ctx);
}

fn execute(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    objective_destination: &Destination,
    candidate: Candidate,
    state: &mut PlayerbotsRunner,
    now: i64,
) {
    match candidate.id.action {
        Action::Hold => {
            if matches!(
                candidate.id.reason,
                Reason::Restricted | Reason::Survival | Reason::CrowdControl
            ) {
                stop(ctx, me.guid, state);
            }
            if candidate.id.reason == Reason::ReturnHome
                && (
                    objective_destination.map_id,
                    objective_destination.instance_id,
                ) != (me.map_id, me.instance_id)
                && state
                    .objective
                    .as_ref()
                    .is_some_and(|o| o.stage != ObjectiveStage::Deferred)
            {
                state.failure(Failure::DestinationUnavailable, now);
                defer(state, now);
                return;
            }
            state.last_outcome = if state
                .objective
                .as_ref()
                .is_some_and(|o| o.stage == ObjectiveStage::Completed)
            {
                RunnerOutcome::Arrived
            } else {
                RunnerOutcome::Waiting
            };
        }
        Action::Move(target) => {
            let destination = match target {
                MoveTarget::Home => {
                    if candidate.id.reason == Reason::Survival
                        && state
                            .objective
                            .as_ref()
                            .is_some_and(|objective| objective.kind == ObjectiveKind::Quest)
                    {
                        super::quest_catalog::retained(ctx, me.guid)
                            .and_then(|retained| super::quest_loop::safe_position(me, &retained))
                            .map(|safe| Destination {
                                map_id: safe.map_id,
                                instance_id: safe.instance_id,
                                x: safe.x,
                                y: safe.y,
                                z: safe.z,
                                geometry_revision: geometry_revision(ctx, safe.map_id),
                            })
                    } else {
                        Some(objective_destination.clone())
                    }
                }
                MoveTarget::Entity(guid) | MoveTarget::CastingPosition(guid) => ctx
                    .db
                    .game_world_entity()
                    .guid()
                    .find(guid)
                    .map(|e| Destination {
                        map_id: e.map_id,
                        instance_id: e.instance_id,
                        x: e.x,
                        y: e.y,
                        z: e.z,
                        geometry_revision: geometry_revision(ctx, e.map_id),
                    }),
                MoveTarget::GameObject(guid) => {
                    ctx.db
                        .game_gameobject()
                        .guid()
                        .find(guid)
                        .map(|go| Destination {
                            map_id: go.map_id,
                            instance_id: go.instance_id,
                            x: go.x,
                            y: go.y,
                            z: go.z,
                            geometry_revision: geometry_revision(ctx, go.map_id),
                        })
                }
            };
            let Some(dest) =
                destination.filter(|d| (d.map_id, d.instance_id) == (me.map_id, me.instance_id))
            else {
                state.failure(Failure::DestinationUnavailable, now);
                if state
                    .objective
                    .as_ref()
                    .is_some_and(|objective| objective.kind != ObjectiveKind::Companion)
                {
                    defer(state, now);
                }
                return;
            };
            if candidate.id.reason == Reason::Survival {
                let _ = crate::actor::stop_attack(ctx, me.guid);
            }
            if now.saturating_sub(state.last_stall_check_micros) >= STALL_INTERVAL {
                state.failure(Failure::NoMovement, now);
                state.last_stall_check_micros = now;
                if state.retry_count >= 3 {
                    stop(ctx, me.guid, state);
                    if state
                        .objective
                        .as_ref()
                        .is_some_and(|objective| objective.kind != ObjectiveKind::Companion)
                    {
                        defer(state, now);
                    }
                    return;
                }
            }
            super::goals::walk_toward(
                ctx,
                me,
                (dest.x, dest.y, dest.z),
                if target == MoveTarget::Home {
                    2.0
                } else if candidate.id.reason == Reason::FightPosition {
                    25.0
                } else {
                    3.0
                },
                true,
            );
            state.route_expansions =
                super::actions::observation(ctx, me.guid, super::actions::ActionKind::Move)
                    .and_then(|row| match row.outcome {
                        super::actions::ActionOutcome::Movement(observation) => {
                            Some(observation.route.expansions)
                        }
                        _ => None,
                    })
                    .unwrap_or(0);
            state.foreground = Some(Foreground {
                candidate,
                generation: state.generation,
                map_id: me.map_id,
                instance_id: me.instance_id,
                started_micros: now,
                running: Running::Movement(MovementRun {
                    destination: dest,
                    from_x: me.x,
                    from_y: me.y,
                }),
            });
            state.last_outcome = RunnerOutcome::Waiting;
        }
        Action::Cast(CastAction { target, spell }) => {
            stop_movement(ctx, me.guid);
            let _ = crate::actor::stop_attack(ctx, me.guid);
            match super::actions::cast(ctx, me.guid, spell, target) {
                Ok(
                    crate::spell::CastStart::Started(handle)
                    | crate::spell::CastStart::Waiting(handle),
                ) => {
                    let mut actual = candidate;
                    actual.id.action = Action::Cast(CastAction {
                        spell: handle.spell_id,
                        target: handle.target_guid,
                    });
                    state.foreground = Some(Foreground {
                        candidate: actual,
                        generation: state.generation,
                        map_id: me.map_id,
                        instance_id: me.instance_id,
                        started_micros: now,
                        running: Running::Cast(handle),
                    });
                    state.last_outcome = RunnerOutcome::Waiting;
                }
                Ok(crate::spell::CastStart::Resolved) => {
                    state.cast_progress = Some(CastProgress {
                        observed_micros: now,
                        scheduled_id: 0,
                        spell,
                        target,
                    });
                    state.last_outcome =
                        RunnerOutcome::CastFinished(crate::spell::CastFinish::Resolved);
                    if state.companion_heal_target_guid == Some(target) {
                        state.companion_heal_target_guid = None;
                    }
                    if state.companion_buff_target_guid == Some(target) {
                        state.companion_buff_target_guid = None;
                    }
                    state.retry_candidate = None;
                }
                Err(reason) => {
                    state.failure(Failure::CastRefused(reason.kind), now);
                    if state.companion_heal_target_guid == Some(target) {
                        state.companion_heal_target_guid = None;
                    }
                    if state.companion_buff_target_guid == Some(target) {
                        state.companion_buff_target_guid = None;
                    }
                    state.retry_candidate = Some(candidate.id);
                    state.next_eligible_micros = now.saturating_add(if state.retry_count >= 3 {
                        DEFER_INTERVAL
                    } else {
                        INTERVAL * i64::from(state.retry_count)
                    });
                }
            }
        }
        Action::Attack(target) => {
            if let Some(target) = ctx.db.game_world_entity().guid().find(target) {
                state.last_target_health = Some(CombatProgress {
                    observed_micros: now,
                    target: target.guid,
                    health: target.health,
                });
            }
            match super::actions::attack(ctx, me.guid, target) {
                Ok(_) => state.last_outcome = RunnerOutcome::Accepted,
                Err(reason) => state.failure(Failure::ActionRefused(reason.kind), now),
            }
        }
        Action::Resurrect => {
            stop(ctx, me.guid, state);
            let result = if me.player_flags & lyracore_shared::constants::player_flags::GHOST != 0 {
                crate::actor::spirit_res(ctx, me.guid)
            } else {
                crate::actor::repop(ctx, me.guid)
            };
            match result {
                Ok(()) => state.last_outcome = RunnerOutcome::Accepted,
                Err(_) => state.failure(
                    Failure::ActionRefused(crate::actor::ActionRefusalKind::Other),
                    now,
                ),
            }
        }
        action @ (Action::AcceptQuest(_)
        | Action::TurnInQuest(_)
        | Action::LootCreature(_)
        | Action::UseGameObject(_)
        | Action::LootGameObject(_)) => match super::quest_loop::execute(ctx, me.guid, action) {
            super::quest_loop::StepResult::Completed => {
                state.last_outcome = RunnerOutcome::Accepted;
                state.retry_count = 0;
                if let Some(objective) = &mut state.objective {
                    objective.last_verified_progress_micros = Some(now);
                }
            }
            super::quest_loop::StepResult::Waiting => {
                state.last_outcome = RunnerOutcome::Waiting;
            }
            super::quest_loop::StepResult::Refused(reason) => {
                state.failure(Failure::ActionRefused(reason), now);
            }
        },
    }
}

crate::game_hook!(on_cast_finished, fn playerbots_runner_cast_finished(ctx, payload) {
    let Some(bot) = ctx.db.pkg_playerbots_bot().by_character().filter(payload.caster_guid).next() else { return; };
    if bot.controller != Controller::Cohort { return; }
    let Some(mut state) = ctx.db.pkg_playerbots_runner().character_guid().find(payload.caster_guid) else { return; };
    let Some(fg) = &state.foreground else { return; };
    let Running::Cast(handle) = &fg.running else { return; };
    if fg.generation != state.generation || handle.scheduled_id != payload.scheduled_id { return; }
    if !ctx.db.game_world_entity().guid().find(payload.caster_guid).is_some_and(|e| (e.map_id, e.instance_id) == (fg.map_id, fg.instance_id)) { return; }
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    if matches!(payload.outcome, crate::spell::CastFinish::Resolved) {
        state.cast_progress = Some(CastProgress { observed_micros: now, scheduled_id: handle.scheduled_id, spell: handle.spell_id, target: handle.target_guid });
        state.retry_candidate = None;
    }
    state.foreground = None;
    if state.companion_heal_target_guid == Some(payload.target_guid) {
        state.companion_heal_target_guid = None;
    }
    if state.companion_buff_target_guid == Some(payload.target_guid) {
        state.companion_buff_target_guid = None;
    }
    state.last_stall_check_micros = now;
    state.last_outcome = RunnerOutcome::CastFinished(payload.outcome.clone());
    state.save(ctx);
});

crate::game_hook!(on_damage_taken, fn playerbots_runner_reconsider_damage(ctx, payload) {
    if payload.attacker_guid == 0 { return; }
    let Some(mut bot) = ctx.db.pkg_playerbots_bot().by_character().filter(payload.target_guid).next() else { return; };
    if !matches!(bot.controller, Controller::Cohort | Controller::RecordOnly) { return; }
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let mut state = ctx.db.pkg_playerbots_runner().character_guid().find(payload.target_guid)
        .unwrap_or_else(|| PlayerbotsRunner::initial(payload.target_guid, now));
    state.defense_target = Some(payload.attacker_guid);
    state.save(ctx);
    bot.next_think_micros = bot.next_think_micros.min(now);
    ctx.db.pkg_playerbots_bot().id().update(bot);
});

// Both active controllers use this one invite response. Sharded admission belongs to the Gateway.
crate::game_hook!(on_group_invite, fn playerbots_auto_accept(ctx, payload) {
    let Some(bot) = ctx.db.pkg_playerbots_bot().by_character().filter(payload.target_guid).next() else { return; };
    if !matches!(bot.controller, Controller::Legacy | Controller::Cohort)
        || crate::actor::sessionless_action_gate(ctx, payload.target_guid).is_err()
    {
        return;
    }
    let inviter = crate::helpers::character_by_guid(ctx, payload.inviter_guid)
        .map(|character| character.name)
        .unwrap_or_else(|| payload.inviter_guid.to_string());
    match crate::actor::accept_group_invite(ctx, payload.target_guid) {
        Ok(()) => spacetimedb::log::info!(
            "playerbots: bot {} accepted a party invite from {inviter}",
            payload.target_guid
        ),
        Err(refusal) => spacetimedb::log::warn!(
            "playerbots: bot {} could not accept the invite from {inviter}: {refusal}",
            payload.target_guid
        ),
    }
});
