//! Solo target selection on private Standalone Shards, including the approach before a Loot Tag.

mod support;
use support::Standalone;

fn verify(case: &str) {
    let mut node = Standalone::start("playerbots-target-claims");
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    node.assert_call("playerbots_fixture_solo_target_claim", &[case]);
}

macro_rules! case {
    ($name:ident, $case:literal) => {
        #[test]
        #[ignore = "requires the pinned Standalone and Module Wasm"]
        fn $name() {
            verify($case);
        }
    };
}

case!(
    solo_bots_approach_distinct_creatures_and_keep_their_choices,
    "split"
);
case!(solo_bot_defers_when_the_only_creature_is_claimed, "wait");
case!(
    solo_population_claims_every_target_before_the_remaining_bots_hold,
    "exhaustion"
);
case!(self_defense_ignores_another_solo_bots_claim, "defense");
case!(party_members_can_select_the_same_creature, "party");
case!(
    joining_a_party_releases_the_owners_solo_claim,
    "party_owner"
);
case!(dead_owner_releases_its_claim, "death");
case!(frozen_owner_releases_its_claim, "freeze");
case!(leaving_the_partition_releases_its_claim, "partition");
case!(unobserved_claim_expires, "expired");
case!(stalled_claim_expires_despite_recent_decisions, "stalled");
case!(progress_keeps_an_old_approach_claimed, "progress");
case!(abandoning_fight_work_releases_its_claim, "abandoned");
case!(failed_path_releases_its_claim, "refused");
case!(
    original_owner_keeps_its_claim_when_defending_itself,
    "owner_defense"
);
case!(
    crowded_camp_does_not_block_new_or_retained_solo_targets,
    "crowd"
);
case!(
    existing_approach_is_indexed_before_another_bot_can_claim_it,
    "backfill"
);

#[test]
#[ignore = "requires the pinned Standalone and both Module revisions"]
fn additive_claim_index_preserves_existing_runner_state() {
    let baseline_path = std::env::var("PLAYERBOTS_CLAIM_BASELINE_WASM")
        .expect("the claim migration check requires its baseline Wasm");
    let baseline = std::fs::read(baseline_path).expect("baseline Wasm missing");
    let mut node = Standalone::start("playerbots-claim-migration");
    node.publish_module_bytes(&baseline);
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    node.assert_sql("DELETE FROM game_creature_move_schedule");
    node.assert_call(
        "playerbots_spawn_class_role",
        &["18", "1200", "1200", "50", "1", "0"],
    );
    let bots = node.query_rows("SELECT character_guid FROM pkg_playerbots_bot");
    let guid = &bots[0]["character_guid"];
    node.assert_call(
        "playerbots_fixture_class_stage",
        &[guid, &bots[17]["character_guid"], "false", "false"],
    );
    for bot in &bots[1..17] {
        node.assert_call(
            "playerbots_select_controller",
            &[&bot["character_guid"], r#"{"frozen":[]}"#],
        );
    }
    node.assert_call("playerbots_fixture_runner_pass_once", &[guid]);
    let retained = format!("SELECT character_guid, generation, objective, chosen, recovery FROM pkg_playerbots_runner WHERE character_guid = {guid}");
    let before = node.query_rows(&retained);
    assert_eq!(before.len(), 1);
    node.publish_module();
    assert_eq!(
        node.query_rows(&retained),
        before,
        "additive publish changed retained bot work"
    );
    let pending = format!(
        "SELECT character_guid FROM pkg_playerbots_runner WHERE solo_target_guid = {}",
        u64::MAX
    );
    assert_eq!(node.query_rows(&pending).len(), 17);
    node.assert_call("playerbots_fixture_runner_pass", &[]);
    assert_eq!(
        node.query_rows(&pending).len(),
        1,
        "one pass exceeded the 16-row backfill budget"
    );
    node.assert_call("playerbots_fixture_runner_pass", &[]);
    assert!(
        node.query_rows(&pending).is_empty(),
        "second backfill pass did not finish"
    );
    assert_eq!(
        node.query_rows(&retained),
        before,
        "backfill changed retained bot work"
    );
    assert_eq!(
        node.query_rows(&format!(
            "SELECT solo_target_guid FROM pkg_playerbots_runner WHERE character_guid = {guid}"
        ))[0]["solo_target_guid"],
        "0"
    );
}
