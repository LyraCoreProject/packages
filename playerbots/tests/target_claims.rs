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
