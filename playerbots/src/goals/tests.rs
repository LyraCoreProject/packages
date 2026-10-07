#![cfg(test)]

use crate::package_test::{
    ask_offline, code_of, read_scanned, shape_of, EntityView, RuntimeScript,
};

#[test]
fn a_pick_stays_within_the_nearest_few_and_differs_between_bots() {
    assert_eq!(pick_index(7, 0), None);
    for salt in 0..64u64 {
        assert!(pick_index(salt, 3).unwrap() < 3);
    }
    let picks: std::collections::BTreeSet<usize> =
        (0..64u64).filter_map(|salt| pick_index(salt, 3)).collect();
    assert_eq!(
        picks.len(),
        3,
        "sixty-four bots must not all choose the same one"
    );
}

#[test]
fn a_quest_is_in_reach_only_when_its_ender_targets_and_droppers_stand_inside_the_leash() {
    let reach: std::collections::BTreeSet<u32> = [51000, 51003].into_iter().collect();
    let drops = |entry: u32, item: u32| entry == 51000 && item == 750;
    assert!(quest_in_reach(&[51000], &[], true, &reach, drops));
    assert!(
        quest_in_reach(&[], &[750], true, &reach, drops),
        "a wolf here drops it"
    );
    assert!(
        !quest_in_reach(&[51000], &[], false, &reach, drops),
        "the ender is elsewhere"
    );
    assert!(
        !quest_in_reach(&[51000, 51002], &[], true, &reach, drops),
        "a target is elsewhere"
    );
    assert!(
        !quest_in_reach(&[], &[2999], true, &reach, drops),
        "nothing here drops it"
    );
}

#[test]
fn a_bot_at_the_interact_boundary_takes_a_real_step() {
    let at_boundary = 5.000_033_f32;
    let step = step_length(
        at_boundary,
        INTERACT_STAND_OFF_YD,
        lyracore_shared::constants::speeds::RUN,
        THINK_INTERVAL_MICROS,
    );
    assert!(
        step >= 1.0,
        "step {step} is below what f32 resolves at |x| near 9000"
    );
    assert!(INTERACT_STAND_OFF_YD < INTERACT_RANGE_YD);
}

use super::*;

#[test]
fn a_flee_threshold_of_zero_never_breaks_off() {
    assert!(!should_flee(1, 100, 0));
    assert!(!should_flee(0, 100, 0));
}

#[test]
fn a_bot_breaks_off_at_or_below_its_own_threshold() {
    assert!(should_flee(15, 100, 15));
    assert!(should_flee(10, 100, 15));
    assert!(!should_flee(16, 100, 15));
}

/// The party-brains divergence in one assertion: same health, same rotation, different bot.
#[test]
fn two_bots_at_the_same_health_diverge_on_the_flee_threshold_alone() {
    let (health, max_health) = (20, 100);
    assert!(!should_flee(health, max_health, 0));
    assert!(should_flee(health, max_health, 95));
}

#[test]
fn an_arrived_follower_emits_no_leg() {
    assert_eq!(step_length(8.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 0.0);
    assert_eq!(step_length(3.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 0.0);
}

#[test]
fn a_leg_never_overshoots_its_stand_off() {
    assert_eq!(step_length(10.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 2.0);
}

#[test]
fn a_long_chase_is_capped_at_one_intervals_travel() {
    assert_eq!(step_length(500.0, 8.0, 7.0, THINK_INTERVAL_MICROS), 7.0);
}

/// The follow acceptance is "within 15 yards after a ~78 yard hop, inside 30 seconds". The step
/// arithmetic has to make that reachable before any live run can.
#[test]
fn a_seventy_eight_yard_hop_converges_well_inside_the_follow_range() {
    let mut distance = 78.0_f32;
    let mut ticks = 0;
    while distance > FOLLOW_RANGE_YD && ticks < 30 {
        distance -= step_length(
            distance,
            FOLLOW_STAND_OFF_YD,
            lyracore_shared::constants::speeds::RUN,
            THINK_INTERVAL_MICROS,
        );
        ticks += 1;
    }
    assert!(
        distance <= FOLLOW_RANGE_YD,
        "a follow that cannot close 78 yards in 30 thinks cannot pass its acceptance: \
         {distance} yards left after {ticks} thinks"
    );
}

#[test]
fn a_wander_never_strays_past_its_radius() {
    for guid in [1_u64, 42, 9_999_999_999, u64::MAX] {
        for window in 0..64_i64 {
            let (dx, dy) = wander_offset(guid, window);
            let reach = (dx * dx + dy * dy).sqrt();
            assert!(
                reach <= WANDER_RADIUS_YD + 0.001,
                "guid {guid} window {window} wandered {reach} yards from home"
            );
        }
    }
}

#[test]
fn a_bot_holds_one_heading_for_a_whole_leg_and_then_changes_it() {
    assert_eq!(wander_offset(7, 3), wander_offset(7, 3));
    assert_ne!(wander_offset(7, 3), wander_offset(7, 4));
}

#[test]
fn two_bots_on_one_home_point_do_not_walk_in_step() {
    assert_ne!(wander_offset(7, 3), wander_offset(8, 3));
}

#[test]
fn an_immobile_bot_travels_nothing() {
    assert_eq!(step_length(50.0, 0.0, 0.0, THINK_INTERVAL_MICROS), 0.0);
}

// ---- crossing a Shard boundary -----------------------------------------------------------

/// The open world of the Shard a party sets out from, and the Deadmines instance they walk
/// into. `HOME` is where a bot was spawned, which is where it goes when there is nothing left
/// to follow.
const HOME: Partition = (0, 0);
const DUNGEON: Partition = (36, 7);

#[test]
fn a_bot_whose_party_walked_into_a_dungeon_crosses_after_them() {
    assert_eq!(
        plan_crossing(HOME, Some(DUNGEON), HOME, 0),
        Crossing::Join(DUNGEON)
    );
}

/// A companion already beside its leader stays there even when both are away from its home.
#[test]
fn a_bot_already_in_its_partys_instance_asks_for_no_crossing() {
    assert_eq!(
        plan_crossing(DUNGEON, Some(DUNGEON), HOME, 0),
        Crossing::Stay
    );
}

/// The return leg. The Shard that serves a dungeon holds the instance under no party, so a bot
/// inside one reads no party instance at all — and once the leader is gone from it too, home is
/// the only place left.
#[test]
fn a_bot_left_in_a_dungeon_goes_home_once_the_wait_is_over() {
    assert_eq!(
        plan_crossing(DUNGEON, None, HOME, STRANDED_WAIT_MICROS),
        Crossing::GoHome
    );
}

/// The arrival window: a bot is driven across on its own, so it can land a moment before the
/// leader whose crossing was driven first. Turning round immediately would be a loop.
#[test]
fn a_bot_that_has_just_arrived_waits_for_its_party_rather_than_turning_round() {
    assert_eq!(
        plan_crossing(DUNGEON, None, HOME, STRANDED_WAIT_MICROS - 1),
        Crossing::Wait
    );
}

#[test]
fn a_bot_standing_on_its_own_home_ground_never_crosses() {
    assert_eq!(plan_crossing(HOME, None, HOME, 0), Crossing::Stay);
    assert_eq!(
        plan_crossing(HOME, None, HOME, STRANDED_WAIT_MICROS * 100),
        Crossing::Stay
    );
}

/// Two parties in two instances of one map are two destinations, so the instance has to be part
/// of the comparison — a bot in instance 7 whose party is in instance 8 has to cross.
#[test]
fn two_instances_of_one_map_are_two_destinations() {
    assert_eq!(
        plan_crossing(DUNGEON, Some((36, 8)), HOME, 0),
        Crossing::Join((36, 8))
    );
}

/// A populated crossing from before durable intents can leave a bodiless bot marked in transit
/// with no Intent row. Its old bounded wait still puts it back in the world.
#[test]
fn a_bot_whose_crossing_was_never_driven_is_put_back_in_the_world() {
    assert!(!may_rebuild(false, Some(goal::IN_TRANSIT), 0));
    assert!(!may_rebuild(
        false,
        Some(goal::IN_TRANSIT),
        IN_TRANSIT_WAIT_MICROS - 1
    ));
    assert!(may_rebuild(
        false,
        Some(goal::IN_TRANSIT),
        IN_TRANSIT_WAIT_MICROS
    ));
}

#[test]
fn a_durable_transfer_intent_prevents_timed_body_rebuild() {
    assert!(!may_rebuild(
        true,
        Some(goal::IN_TRANSIT),
        IN_TRANSIT_WAIT_MICROS * 100
    ));
}

/// Arrival adoption: a Transfer does not carry the goal row, so an arriving bot holds no goal
/// at all — and that is the state the tick has to rebuild a body for, immediately.
#[test]
fn a_bot_that_arrives_with_no_goal_is_rebuilt_at_once() {
    assert!(may_rebuild(false, None, 0));
}

/// A bodiless bot that is not crossing is a bot whose Shard despawned it — the same rebuild,
/// with no wait to serve.
#[test]
fn a_bodiless_bot_holding_any_other_goal_is_rebuilt_at_once() {
    for kind in [
        goal::FOLLOW,
        goal::FIGHT,
        goal::FLEE,
        goal::WANDER,
        goal::STRANDED,
    ] {
        assert!(may_rebuild(false, Some(kind), 0), "goal kind {kind}");
    }
}

// ---- getting back up -----------------------------------------------------------------------

#[test]
fn a_living_bot_has_no_death_to_recover_from() {
    assert_eq!(death_step(false, false), DeathStep::None);
    assert_eq!(death_step(false, true), DeathStep::None);
}

/// The whole recovery, in the order it happens: a fresh corpse releases, and the ghost that
/// release produced resurrects. Two ticks, and the bot is standing.
#[test]
fn a_dead_bot_releases_and_then_resurrects() {
    assert_eq!(death_step(true, false), DeathStep::Release);
    assert_eq!(death_step(true, true), DeathStep::Resurrect);
}

// ---- who swings ----------------------------------------------------------------------------

#[test]
fn a_healer_in_a_party_stays_back_and_everyone_else_closes() {
    assert!(!closes_to_melee(ROLE_HEALER, true));
    assert!(closes_to_melee(ROLE_TANK, true));
}

/// A solo healer that would not swing kills nothing, so it finishes no quest objective and
/// holds one goal for the rest of its life.
#[test]
fn an_ungrouped_healer_fights_like_anyone_else() {
    assert!(closes_to_melee(ROLE_HEALER, false));
}

// ---- Loot Tag target policy ---------------------------------------------------------------

#[test]
fn an_untagged_live_target_is_available() {
    assert!(live_target_is_available_to(17, None));
}

#[test]
fn an_entitled_character_can_take_a_live_target() {
    assert!(live_target_is_available_to(17, Some(&[11, 17, 23])));
}

#[test]
fn a_foreign_live_target_is_unavailable() {
    assert!(!live_target_is_available_to(17, Some(&[11, 23])));
}

#[test]
fn a_foreign_current_melee_target_is_stopped() {
    // ReducerContext has no unit-test Fake. The pure tests above pin the decision; this narrow
    // source check pins the actor chokepoint that applies it.
    let shape = shape_of(include_str!("../goals.rs"), "fn current_melee_target(");
    assert!(shape.ends_with(
        "if live_target_is_available(ctx, character_guid, &target) { return Some(target_guid); \
         } let _ = crate::actor::stop_attack(ctx, character_guid); None }"
    ));
}

#[test]
fn a_friendly_rotation_target_bypasses_hostile_ownership() {
    let mut checked_ownership = false;
    let cast_at = available_rotation_target(RotationTarget::Friendly(17), |_| {
        checked_ownership = true;
        false
    });

    assert_eq!(cast_at, Some(17));
    assert!(!checked_ownership);
}

#[test]
fn a_hostile_rotation_target_requires_live_target_ownership() {
    assert_eq!(
        available_rotation_target(RotationTarget::Hostile(91), |_| false),
        None
    );
    assert_eq!(
        available_rotation_target(RotationTarget::Hostile(91), |_| true),
        Some(91)
    );
}

// ---- corpse loot -------------------------------------------------------------------------

#[test]
fn an_eligible_corpse_is_considered_for_loot() {
    assert!(crate::loot::corpse_eligible_for_access(&[11, 17, 23], 17));
}

#[test]
fn a_foreign_corpse_is_skipped_before_loot_work() {
    assert!(!crate::loot::corpse_eligible_for_access(&[11, 23], 17));
}

#[test]
fn an_eligible_corpse_yields_money_and_wanted_items() {
    let mut actions = Vec::new();
    let result = loot_eligible_corpse(
        true,
        || vec![3, 8],
        || true,
        |action| {
            actions.push(action);
            Ok(())
        },
    );

    assert_eq!(result, CorpseLootResult::NextCorpse);
    assert_eq!(
        actions,
        vec![
            CorpseLootAction::Money,
            CorpseLootAction::Item(3),
            CorpseLootAction::Item(8),
        ]
    );
}

#[test]
fn a_money_loot_tag_refusal_suppresses_item_work() {
    let mut actions = Vec::new();
    let mut read_wanted_slots = false;
    let result = loot_eligible_corpse(
        true,
        || {
            read_wanted_slots = true;
            vec![3, 8]
        },
        || true,
        |action| {
            actions.push(action);
            Err("loot_tag_ineligible: actor_guid=17 corpse_guid=91".to_owned())
        },
    );

    assert_eq!(result, CorpseLootResult::NextCorpse);
    assert_eq!(actions, vec![CorpseLootAction::Money]);
    assert!(!read_wanted_slots);
}

#[test]
fn an_item_loot_tag_refusal_suppresses_later_slots() {
    let mut actions = Vec::new();
    let result = loot_eligible_corpse(
        true,
        || vec![3, 8],
        || true,
        |action| {
            actions.push(action);
            match action {
                CorpseLootAction::Item(3) => {
                    Err("loot_tag_ineligible: actor_guid=17 corpse_guid=91".to_owned())
                }
                _ => Ok(()),
            }
        },
    );

    assert_eq!(result, CorpseLootResult::NextCorpse);
    assert_eq!(
        actions,
        vec![CorpseLootAction::Money, CorpseLootAction::Item(3)]
    );
}

#[test]
fn only_the_exact_loot_tag_refusal_prefix_matches() {
    assert!(is_loot_tag_refusal(
        "loot_tag_ineligible: actor_guid=17 corpse_guid=91"
    ));
    assert!(!is_loot_tag_refusal(
        "action failed: loot_tag_ineligible: actor_guid=17 corpse_guid=91"
    ));
    assert!(!is_loot_tag_refusal(
        "loot_tag_ineligibleish: actor_guid=17 corpse_guid=91"
    ));
}

#[test]
fn another_loot_refusal_keeps_the_existing_best_effort_work() {
    let mut actions = Vec::new();
    let result = loot_eligible_corpse(
        true,
        || vec![3],
        || true,
        |action| {
            actions.push(action);
            Err("inventory_full".to_owned())
        },
    );

    assert_eq!(result, CorpseLootResult::NextCorpse);
    assert_eq!(
        actions,
        vec![CorpseLootAction::Money, CorpseLootAction::Item(3)]
    );
}

// ---- the one Gate the Package still answers itself -------------------------------------------

/// A bot loots every corpse it makes and never sells, so its bag fills and stays full. A quest
/// that hands an item over on accept is refused by the core on a full bag — from inside the
/// accept EFFECTS, past every Gate `accept_gates` applies. A selection that could not see that
/// would walk to the giver, be refused, walk away, and walk back, once a second.
#[test]
fn a_quest_that_hands_over_an_item_is_never_chosen_with_a_full_bag() {
    assert!(!bag_can_take_the_quest_item(true, false));
    assert!(bag_can_take_the_quest_item(true, true));
}

/// Most quests hand nothing over, and a full bag must not stop a bot taking one of those.
#[test]
fn a_full_bag_does_not_stop_a_quest_that_hands_nothing_over() {
    assert!(bag_can_take_the_quest_item(false, false));
}

// ---- what a bot picks a fight with ---------------------------------------------------------

/// The scenario fixture pins the band: Test Wolf Elder is level 8 against a level 10 bot,
/// "inside the goals.rs GRIND ±3 band, non-grey".
#[test]
fn the_grind_band_takes_the_fixtures_own_worked_example() {
    assert!(worth_grinding(10, 8, false));
}

/// Grey is the core's own kill-XP clamp: six levels down pays nothing, so there is no reason
/// to swing at it.
#[test]
fn a_grey_creature_is_not_worth_grinding() {
    assert!(worth_grinding(10, 5, false));
    assert!(!worth_grinding(10, 4, false));
}

#[test]
fn a_creature_more_than_three_levels_up_is_left_alone() {
    assert!(worth_grinding(10, 13, false));
    assert!(!worth_grinding(10, 14, false));
}

/// A tank opens with a flee threshold of 0, so it never breaks off. Left to pick its own
/// fights it would walk into the nearest elite, die, resurrect at the graveyard, walk back
/// into it, and spend its life doing that.
#[test]
fn an_elite_is_never_grind_bait_however_low_it_is() {
    assert!(!worth_grinding(10, 10, true));
    assert!(!worth_grinding(10, 8, true));
}

// ---- what a bot can finish -----------------------------------------------------------------

use crate::quest::objective_kind::{
    COLLECT_ITEM, EXPLORE_AREATRIGGER, KILL_CREATURE, USE_GAMEOBJECT,
};

/// A gameobject used and a place explored are credited from a message a client sends, and a bot
/// has no client. A quest made only of those can never be finished, and taking one costs the
/// bot a third of its attention for the rest of its life.
#[test]
fn a_quest_a_session_less_bot_can_never_credit_is_never_taken() {
    assert!(!objectives_are_workable(&[USE_GAMEOBJECT], false));
    assert!(!objectives_are_workable(&[EXPLORE_AREATRIGGER], false));
    assert!(!objectives_are_workable(
        &[USE_GAMEOBJECT, EXPLORE_AREATRIGGER],
        false
    ));
}

/// The talk-to-somebody quest that opens most chains has no objectives at all and is complete
/// the moment it is accepted. Reading "no objectives" as "nothing I can do" would stop a bot at
/// the first step of every chain on the realm.
#[test]
fn a_quest_with_no_objectives_is_taken_and_handed_straight_back() {
    assert!(objectives_are_workable(&[], false));
}

#[test]
fn the_two_kinds_a_bot_works_are_taken() {
    assert!(objectives_are_workable(&[KILL_CREATURE], false));
    assert!(objectives_are_workable(&[COLLECT_ITEM], false));
}

/// A quest that mixes something the bot can do with something it cannot is still taken. Part of
/// it is worth watching, and the stall clock is what records that it never finishes — the skip
/// above is for quests where there was never anything to watch.
#[test]
fn a_quest_the_bot_can_partly_do_is_still_taken() {
    assert!(objectives_are_workable(
        &[KILL_CREATURE, EXPLORE_AREATRIGGER],
        false
    ));
}

/// An event requirement is credited by an EventAI action on a creature the objective rows never
/// name, so the bot has nothing to aim at. `quest_is_complete` refuses to call the quest
/// complete without that credit, and the zero-objective shape is the one that shows: the bot
/// takes a quest it reads as complete-on-accept and then holds it for good.
#[test]
fn a_quest_that_needs_an_event_credit_is_never_taken() {
    assert!(!objectives_are_workable(&[], true));
    assert!(!objectives_are_workable(&[KILL_CREATURE], true));
    assert!(!objectives_are_workable(&[COLLECT_ITEM], true));
}

// ---- saying so when a bot is stuck ---------------------------------------------------------

/// The clock opens on the first tick a bot with quests in its log gets nowhere.
#[test]
fn the_clock_opens_when_a_bot_first_gets_nowhere() {
    let opened = tick_stall(Stall::default(), QuestWork::NoProgress, true, 5_000);
    assert_eq!(opened.stall.since_micros, 5_000);
    assert!(!opened.warn, "one tick of nothing is not a stall yet");
}

/// Real quest work is the only thing that stops the clock. A bot holding no quests at all is
/// idle rather than stuck, and has nothing to be stuck on.
#[test]
fn quest_work_and_an_empty_log_both_stop_the_clock() {
    let running = Stall {
        since_micros: 1_000,
        warned: true,
    };
    assert_eq!(
        tick_stall(running, QuestWork::Progress, true, 90_000_000).stall,
        Stall::default()
    );
    assert_eq!(
        tick_stall(running, QuestWork::NoProgress, false, 90_000_000).stall,
        Stall::default()
    );
}

/// THE regression this issue was filed for. The speculative walk back to the quest hub records
/// QUEST_TRAVEL, and the clock used to clear on that goal kind — so a bot flapping between the
/// hub and its grinding ground reset its own evidence on every excursion, roughly twenty
/// seconds against a minute of patience, and the warning never fired in the case the hub
/// bookmark was added for. The clock reads the OUTCOME now, and a walk is not one.
#[test]
fn a_walk_that_gets_nowhere_leaves_the_clock_where_it_was() {
    let running = Stall {
        since_micros: 1_000,
        warned: false,
    };
    assert_eq!(
        tick_stall(running, QuestWork::NoProgress, true, 30_000_000).stall,
        running
    );
}

/// The whole motivating case as the sequence that produced it: five minutes of walking and
/// grinding, never a quest accepted, turned in, or hunted. One warning, and only one.
#[test]
fn a_bot_flapping_between_its_hub_and_its_grinding_ground_warns_once() {
    let mut stall = Stall::default();
    let mut warnings = 0;
    for tick in 0..300_i64 {
        let step = tick_stall(
            stall,
            QuestWork::NoProgress,
            true,
            tick * THINK_INTERVAL_MICROS,
        );
        stall = step.stall;
        warnings += i32::from(step.warn);
    }
    assert_eq!(
        warnings, 1,
        "a bot that gets nowhere for five minutes says so exactly once"
    );
}

/// The latch, against the think gap the window could not survive. A republish or scheduler
/// jitter can leave the bot un-thought-about across the patience mark; the warning is still
/// owed on the next tick, whenever it lands.
#[test]
fn a_think_gap_that_steps_over_the_patience_still_gets_its_warning() {
    let opened_at = 1_000_i64;
    let running = Stall {
        since_micros: opened_at,
        warned: false,
    };
    // The bot was last thought about a second short of the patience, then not again for forty.
    assert!(
        !tick_stall(
            running,
            QuestWork::NoProgress,
            true,
            opened_at + QUEST_STALL_PATIENCE_MICROS - THINK_INTERVAL_MICROS
        )
        .warn
    );
    let after_the_gap = opened_at + QUEST_STALL_PATIENCE_MICROS + THINK_INTERVAL_MICROS * 40;
    assert!(tick_stall(running, QuestWork::NoProgress, true, after_the_gap).warn);
}

/// A stall that has been running for an hour was announced an hour ago. Saying it again every
/// second would bury the drift warnings this loop's other refusals carry.
#[test]
fn a_stall_already_announced_is_not_announced_again() {
    let announced = Stall {
        since_micros: 1,
        warned: true,
    };
    assert!(
        !tick_stall(
            announced,
            QuestWork::NoProgress,
            true,
            QUEST_STALL_PATIENCE_MICROS * 60
        )
        .warn
    );
}

/// The latch is per stall, not per bot. A bot that got going again and then stuck a second time
/// is a second stall, and it is worth saying so.
#[test]
fn a_second_stall_after_real_work_warns_again() {
    let mut stall = Stall::default();
    let mut warnings = 0;
    for tick in 0..300_i64 {
        let now = tick * THINK_INTERVAL_MICROS;
        // One quest turned in half way through: the clock and the latch both reset.
        let work = if tick == 150 {
            QuestWork::Progress
        } else {
            QuestWork::NoProgress
        };
        let step = tick_stall(stall, work, true, now);
        stall = step.stall;
        warnings += i32::from(step.warn);
    }
    assert_eq!(warnings, 2, "two stalls, two warnings");
}

// ---- timed quests --------------------------------------------------------------------------

#[test]
fn an_untimed_quest_never_runs_out() {
    assert!(expired_quest_can_wait(0, i64::MAX));
}

/// The sweep that marks a timed quest failed runs on its own clock, so between the deadline
/// passing and the sweep firing the row still reads active. A bot that read only the flag would
/// set off across the hub for a Refusal in that window.
#[test]
fn a_timed_quest_is_not_carried_back_past_its_deadline() {
    assert!(expired_quest_can_wait(1_000, 999));
    assert!(!expired_quest_can_wait(1_000, 1_000));
    assert!(!expired_quest_can_wait(1_000, 5_000));
}

// ---- personality, as a script or as the row ----------------------------------------------------

/// The point of the whole feature: a script that answers a share is what the bot uses, not the
/// row it was spawned with.
#[test]
fn a_script_answer_is_what_the_bot_uses() {
    assert_eq!(threshold_from(Some(60.0), 15), 60);
    assert_eq!(threshold_from(Some(0.0), 15), 0, "never flee is an answer");
    assert_eq!(threshold_from(Some(100.0), 15), 100, "always flee is too");
}

/// Nothing bound, a script that returned nothing, and a script that returned something that is
/// not a number all reach here as `None`. Each leaves the bot on its row.
#[test]
fn no_answer_leaves_the_bot_on_its_personality_row() {
    assert_eq!(threshold_from(None, 15), 15);
    assert_eq!(threshold_from(None, 0), 0);
}

/// A script that failed on syntax, ran out of Fuel, or raised an error contributes no answer,
/// so a broken script is a bot on its row rather than a bot frozen. This is the acceptance the
/// live-DB test drives end to end.
#[test]
fn a_broken_script_is_a_bot_on_its_row_not_a_bot_stopped() {
    let row = super::super::role_personality_defaults(ROLE_HEALER);
    assert_eq!(threshold_from(None, row.0), row.0);
    assert_eq!(threshold_from(None, row.1), row.1);
}

/// Refused rather than clamped. A script answering 5000 has a bug in it, and clamping that to
/// 100 would make every bot flee at full health while the log said nothing.
#[test]
fn a_number_that_is_not_a_share_is_not_an_answer() {
    assert_eq!(threshold_from(Some(101.0), 15), 15);
    assert_eq!(threshold_from(Some(5_000.0), 15), 15);
    assert_eq!(threshold_from(Some(-1.0), 15), 15);
    assert_eq!(threshold_from(Some(f64::NAN), 15), 15);
    assert_eq!(threshold_from(Some(f64::INFINITY), 15), 15);
}

/// Lua has one number type, so a script doing arithmetic answers with a fraction whether it
/// meant to or not. A share is a whole percent everywhere else this Package reads one.
#[test]
fn a_fractional_answer_truncates_to_a_whole_percent() {
    assert_eq!(threshold_from(Some(15.9), 0), 15);
    assert_eq!(threshold_from(Some(0.9), 50), 0);
}

// ---- the personality scripts this Package ships --------------------------------------------

/// The artifact as it ships, read from the file an Operator reconciles onto a Shard.
const PERSONALITY_ARTIFACT: &str = include_str!("../../data/.generated/personality.json");

fn entity(level: u32, health: u32, max_health: u32) -> EntityView {
    EntityView {
        guid: 1,
        name: "Dpsbot1".to_string(),
        is_player: true,
        level,
        health,
        max_health,
        map_id: 0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    }
}

/// Run the shipped script bound to `event` against one actor/target pair and read its answer,
/// exactly as `script_binding::ask` would.
fn shipped_answer(event: &str, actor: EntityView, target: Option<EntityView>) -> Option<f64> {
    let artifact = lyracore_package_delta::ScriptArtifact::parse(PERSONALITY_ARTIFACT)
        .expect("the shipped artifact parses");
    let bound: Vec<_> = artifact
        .scripts()
        .iter()
        .filter(|script| script.event().as_str() == event)
        .collect();
    assert_eq!(bound.len(), 1, "one script per personality event");
    let scripts = [RuntimeScript {
        name: bound[0].name().as_str(),
        source: bound[0].source(),
    }];
    ask_offline(event, Some(actor), target, &scripts)
        .unwrap_or_else(|diagnostics| panic!("the shipped Lua must run clean: {diagnostics}"))
}

/// The artifact is hand-written, so nothing regenerates it and nothing else would catch a typo
/// before a realm did. This is that check: the identity, the two events, and the reserved range
/// the identifiers have to sit in.
#[test]
fn the_shipped_artifact_is_a_valid_script_artifact_for_this_package() {
    let artifact = lyracore_package_delta::ScriptArtifact::parse(PERSONALITY_ARTIFACT)
        .expect("the shipped artifact parses");
    assert_eq!(artifact.package().as_str(), super::super::PACKAGE);
    let bound: Vec<(u32, &str, bool)> = artifact
        .scripts()
        .iter()
        .map(|script| {
            (
                script.script_id(),
                script.event().as_str(),
                script.enabled(),
            )
        })
        .collect();
    assert_eq!(
        bound,
        vec![
            (100_100, FLEE_AT_EVENT, true),
            (100_101, HEAL_AT_EVENT, true)
        ],
        "both events this Package asks are bound, and both ship switched on"
    );
}

/// The acceptance the live-DB test drives on a realm, at the rung that runs everywhere: the
/// shipped Lua answers, and the answer is not the share the bot was spawned with.
#[test]
fn the_flee_script_answers_something_the_personality_row_never_would() {
    let low = shipped_answer(FLEE_AT_EVENT, entity(1, 100, 100), None);
    let high = shipped_answer(FLEE_AT_EVENT, entity(30, 100, 100), None);
    assert_eq!(threshold_from(low, 15), 39, "a level 1 bot bolts early");
    assert_eq!(threshold_from(high, 15), 10, "a level 30 bot holds on");
    for role in [ROLE_TANK, ROLE_HEALER, super::super::ROLE_DPS] {
        let (row_flee, _) = super::super::role_personality_defaults(role);
        assert_ne!(
            threshold_from(low, row_flee),
            row_flee,
            "role {role}'s spawned share must be observably overridden"
        );
    }
}

/// The second half of the same answer: a bot already under half health leaves a little earlier
/// than the one that is not.
#[test]
fn the_flee_script_reads_how_hurt_the_bot_already_is() {
    let healthy = shipped_answer(FLEE_AT_EVENT, entity(10, 100, 100), None);
    let hurt = shipped_answer(FLEE_AT_EVENT, entity(10, 40, 100), None);
    assert_eq!(threshold_from(healthy, 15), 30);
    assert_eq!(threshold_from(hurt, 15), 40);
}

/// The healer's answer is about WHICH ally, because the rotation row's own share is the ceiling
/// — a heal share can only ever be tightened, never loosened. So the member with the bigger
/// pool keeps the row's share and everybody else waits.
#[test]
fn the_heal_script_lets_the_member_taking_the_hits_through_first() {
    let healer = entity(10, 200, 200);
    let tank = shipped_answer(HEAL_AT_EVENT, healer.clone(), Some(entity(10, 300, 400)));
    let other = shipped_answer(HEAL_AT_EVENT, healer, Some(entity(10, 150, 200)));
    assert_eq!(
        threshold_from(tank, 80),
        100,
        "the rotation row's own share stands for the tank"
    );
    assert_eq!(
        threshold_from(other, 80),
        45,
        "everybody else waits until they are properly hurt"
    );
    let (_, row_heal) = super::super::role_personality_defaults(ROLE_HEALER);
    assert_ne!(threshold_from(other, row_heal), row_heal);
}

/// A Package Event fires with whatever the caller had. `playerbots.heal_at` is asked even when
/// the party has nobody to heal, so the script has to survive an absent target rather than
/// failing and costing the healer its row.
#[test]
fn the_heal_script_survives_an_absent_ally() {
    assert_eq!(
        shipped_answer(HEAL_AT_EVENT, entity(10, 200, 200), None),
        None,
        "no ally is no answer, which is the row"
    );
    assert_eq!(threshold_from(None, 80), 80);
}

// ---- serendipity -----------------------------------------------------------------------------

/// The good case, which every case below spoils exactly one field of: another player, alive,
/// close, same team, ungrouped, on a quest this bot is working.
fn a_fellow_quester() -> Neighbour {
    Neighbour {
        guid: 22,
        is_player: true,
        dead: false,
        distance_yd: 12.0,
        same_team: true,
        grouped: false,
        shares_an_active_quest: true,
    }
}

#[test]
fn a_fellow_quester_in_range_is_worth_inviting() {
    assert!(worth_inviting(11, &a_fellow_quester()));
}

/// The sight list holds the bot itself, and `pick_near` is not the only thing that must know it.
#[test]
fn a_bot_never_invites_itself() {
    let me = Neighbour {
        guid: 11,
        ..a_fellow_quester()
    };
    assert!(!worth_inviting(11, &me));
}

#[test]
fn a_creature_or_a_corpse_is_never_invited() {
    assert!(!worth_inviting(
        11,
        &Neighbour {
            is_player: false,
            ..a_fellow_quester()
        }
    ));
    assert!(!worth_inviting(
        11,
        &Neighbour {
            dead: true,
            ..a_fellow_quester()
        }
    ));
}

/// The invite means "we are both working this ground". Forty yards away is that; the far edge
/// of what a bot can see is not.
#[test]
fn a_quester_beyond_the_invite_range_is_left_alone() {
    assert!(worth_inviting(
        11,
        &Neighbour {
            distance_yd: INVITE_RANGE_YD,
            ..a_fellow_quester()
        }
    ));
    assert!(!worth_inviting(
        11,
        &Neighbour {
            distance_yd: INVITE_RANGE_YD + 0.1,
            ..a_fellow_quester()
        }
    ));
}

#[test]
fn the_other_team_is_never_invited() {
    assert!(!worth_inviting(
        11,
        &Neighbour {
            same_team: false,
            ..a_fellow_quester()
        }
    ));
}

/// Somebody already in a party would refuse the invite at the core, so asking is a Refusal a
/// second for as long as they both stand there.
#[test]
fn somebody_already_in_a_party_is_not_invited() {
    assert!(!worth_inviting(
        11,
        &Neighbour {
            grouped: true,
            ..a_fellow_quester()
        }
    ));
}

/// The shared quest is the whole of it. A stranger on no quest of the bot's has nothing to
/// group up about, and a party formed with one would have nothing to do and never end.
#[test]
fn a_stranger_on_no_quest_of_the_bots_is_not_invited() {
    assert!(!worth_inviting(
        11,
        &Neighbour {
            shares_an_active_quest: false,
            ..a_fellow_quester()
        }
    ));
}

/// The window is staggered by guid, so two bots standing on one pad do not look on the same
/// second and hand one neighbour two Intents.
#[test]
fn two_bots_do_not_scan_on_the_same_second() {
    let now = |second: i64| second * THINK_INTERVAL_MICROS;
    let opens_at: Vec<i64> = (0..60)
        .filter(|second| invite_scan_is_open(7, now(*second)))
        .collect();
    let neighbours_open_at: Vec<i64> = (0..60)
        .filter(|second| invite_scan_is_open(8, now(*second)))
        .collect();
    assert!(
        opens_at.iter().all(|s| !neighbours_open_at.contains(s)),
        "guid 7 opened at {opens_at:?} and guid 8 at {neighbours_open_at:?}"
    );
}

/// One bot looks about once every fifteen seconds — often enough to catch a neighbour standing
/// on the same quest, rarely enough that an Intent already in flight has landed.
#[test]
fn one_bot_scans_about_once_every_fifteen_seconds() {
    let opens = (0..600_i64)
        .filter(|second| invite_scan_is_open(7, second * THINK_INTERVAL_MICROS))
        .count();
    assert_eq!(opens, 600 / INVITE_SCAN_SECONDS as usize);
}

/// Every guid gets a window. A stagger that left some bot permanently shut would be a bot that
/// never invites anybody, and nothing else in the loop would say so.
#[test]
fn every_bot_gets_a_window() {
    for guid in [0_u64, 1, 14, 15, 9_999_999_999, u64::MAX] {
        assert!(
            (0..INVITE_SCAN_SECONDS as i64)
                .any(|second| invite_scan_is_open(guid, second * THINK_INTERVAL_MICROS)),
            "guid {guid} never scans"
        );
    }
}

// ---- parting ways ----------------------------------------------------------------------------

/// The party is for the shared work, so it lasts exactly as long as the work does.
#[test]
fn a_leader_with_shared_work_left_keeps_its_party() {
    assert!(!leaves_the_party(true, || true));
}

/// Both quests handed in: the leader leaves, leadership passes, and a party of two disbands —
/// which is what puts both bots back in the population an invite is drawn from.
#[test]
fn a_leader_with_nothing_left_to_share_parts_ways() {
    assert!(leaves_the_party(true, || false));
}

/// Leaving a party it did not form is not a member's decision. It also must not read its
/// party's quest logs to find that out.
#[test]
fn a_member_never_leaves_and_never_asks_whether_to() {
    let mut asked = false;
    let leaves = leaves_the_party(false, || {
        asked = true;
        false
    });
    assert!(!leaves);
    assert!(!asked, "only a leader's answer is ever used");
}

// ---- teardown --------------------------------------------------------------------------------

/// Teardown leaves zero rows. A quester writes two kinds of durable row a wandering bot never
/// did — its quest log, and the corpse a death leaves — and both have to go when the Operator
/// despawns the population. Neither is this Package's table, so this pins that the core sweeps
/// them rather than adding a sweep of our own.
#[test]
fn a_despawn_takes_the_quest_log_and_the_corpse_with_it() {
    assert!(
        crate::CHARACTER_OWNED_TABLES.contains(&"game_character_quest"),
        "a bot's quest log is not swept when its Character is deleted, so despawning the \
         population would leave quest rows behind"
    );
    let src = read_scanned("module/src/world.rs")
        .expect("module/src/world.rs is core, never an optional drop-in");
    let body = code_of(&src, "pub(crate) fn cascade_delete_character(");
    assert!(
        body.contains("game_corpse()"),
        "`cascade_delete_character` no longer deletes the corpse, so a bot despawned as a \
         ghost would leave one standing in a field forever"
    );
}
