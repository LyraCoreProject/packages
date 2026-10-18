mod support;

use support::Defense;

fn defense_with_ranks(name: &str) -> Defense {
    let defense = Defense::start(name);
    let client =
        std::env::var("LYRACORE_TEST_CLIENT_DATA").expect("actual client Data is required");
    let dump = std::env::var("LYRACORE_TEST_WORLD_DUMP").expect("ClassicDB SQL dump is required");
    let output = support::private_importer(
        &defense.node,
        &["--dump", &dump, "--dbc", &client, "--family", "spellmeta"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    defense
}

fn knows(defense: &Defense, index: usize, spell: u32) -> bool {
    !defense
        .node
        .query_rows(&format!(
            "SELECT * FROM game_player_spell WHERE character_guid = {} AND spell_id = {spell}",
            defense.characters[index].guid,
        ))
        .is_empty()
}

fn set_level(defense: &Defense, index: usize, level: u8, talent_spell: u32) {
    let actor: serde_json::Value = serde_json::from_str(&defense.characters[index].actor).unwrap();
    let token = serde_json::to_string(&actor["ownership"]["some"]).unwrap();
    defense.node.assert_call("renew_account_claim", &[&token]);
    defense.node.assert_call(
        "gw_certify_package_party_authority",
        &["\"wave-defense-test\"", "true"],
    );
    defense.node.assert_call(
        "wave_defense_fixture_reward_level",
        &[
            &defense.characters[index].guid,
            &level.to_string(),
            &talent_spell.to_string(),
        ],
    );
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn a_five_character_raid_cannot_join_the_queue() {
    let defense = Defense::start("defense-raid-queue");
    for index in 0..5 {
        defense.select(index);
    }
    let roster = serde_json::to_string(
        &defense.characters[..5]
            .iter()
            .map(|character| character.guid.parse::<u64>().unwrap())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    defense
        .node
        .assert_call("debug_package_form_party", &[&roster]);
    defense.node.assert_call(
        "wave_defense_fixture_eligibility",
        &[&defense.characters[0].guid, "\"raid\""],
    );
    defense.refused(0, "queue", "2", "Raid");
    assert!(defense
        .node
        .query_rows("SELECT * FROM pkg_wave_defense_queue")
        .is_empty());
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn stale_premades_leave_the_queue_without_blocking_an_eligible_team() {
    for condition in [
        "raid",
        "meeting-party",
        "meeting-seeker",
        "taxi",
        "offline",
        "partition",
        "level",
        "health",
        "pending",
        "unselected",
        "membership",
    ] {
        let defense = Defense::start(&format!("defense-stale-{condition}"));
        let node = &defense.node;
        for index in 0..10 {
            defense.select(index);
        }
        let roster = serde_json::to_string(
            &defense.characters[..5]
                .iter()
                .map(|character| character.guid.parse::<u64>().unwrap())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        node.assert_call("debug_package_form_party", &[&roster]);
        for index in 0..10 {
            defense.queue(index);
        }
        node.assert_call(
            "wave_defense_fixture_eligibility",
            &[
                &defense.characters[0].guid,
                &serde_json::to_string(condition).unwrap(),
            ],
        );
        let membership = node.query_rows(&format!(
            "SELECT * FROM game_group_member WHERE character_guid = {}",
            defense.characters[0].guid,
        ));
        node.assert_call("wave_defense_fixture_queue_tick", &[]);
        assert_eq!(
            node.query_rows("SELECT * FROM pkg_wave_defense_match")
                .len(),
            1,
            "{condition}"
        );
        for index in 0..5 {
            assert!(
                node.query_rows(&format!(
                    "SELECT * FROM pkg_wave_defense_member WHERE character_guid = {}",
                    defense.characters[index].guid,
                ))
                .is_empty(),
                "{condition}"
            );
        }
        assert_eq!(
            node.query_rows("SELECT * FROM pkg_wave_defense_member")
                .len(),
            5,
            "{condition}"
        );
        assert!(
            node.query_rows("SELECT * FROM pkg_wave_defense_queue")
                .is_empty(),
            "{condition}"
        );
        assert_eq!(
            node.query_rows(&format!(
                "SELECT * FROM game_group_member WHERE character_guid = {}",
                defense.characters[0].guid,
            )),
            membership,
            "{condition}"
        );
        assert_eq!(defense.run(5)["level"], "1");
    }
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn selected_active_talents_gain_current_and_later_spell_ranks() {
    let defense = defense_with_ranks("defense-talent-ranks");
    defense.start_team(0);
    for (index, (class, talent_spell, rank_at_40, rank_at_60)) in [
        (5, 15237, 15431, 27801),
        (2, 20375, 20918, 20920),
        (8, 11426, 11426, 13033),
    ]
    .into_iter()
    .enumerate()
    {
        defense.node.assert_call(
            "wave_defense_fixture_reward_class",
            &[&defense.characters[2].guid, &class.to_string()],
        );
        set_level(&defense, 2, 40, talent_spell);
        let offer = defense.offer(2);
        defense.command(
            2,
            "choose",
            &format!("{}|{}|{}|{}", index + 3, offer[1], offer[2], offer[5]),
        );
        assert!(knows(&defense, 2, rank_at_40), "class {class} current rank");
        assert!(
            !knows(&defense, 2, rank_at_60),
            "class {class} premature rank"
        );
        set_level(&defense, 2, 60, 0);
        assert!(knows(&defense, 2, rank_at_60), "class {class} later rank");
    }
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn class_proficiencies_arrive_at_their_training_levels() {
    let defense = Defense::start("defense-class-proficiencies");
    defense.start_team(0);
    for (class, spell, level) in [
        (1, 750, 40),
        (2, 750, 40),
        (3, 8737, 40),
        (7, 8737, 40),
        (4, 674, 10),
        (1, 674, 20),
        (3, 674, 20),
        (1, 3127, 6),
        (2, 3127, 8),
        (3, 3127, 8),
        (4, 3127, 12),
        (11, 9634, 40),
        (7, 20608, 30),
    ] {
        defense.node.assert_call(
            "wave_defense_fixture_reward_class",
            &[&defense.characters[0].guid, &class.to_string()],
        );
        set_level(&defense, 0, level - 1, 0);
        assert!(
            !knows(&defense, 0, spell),
            "class {class} premature proficiency {spell}"
        );
        set_level(&defense, 0, level, 0);
        assert!(
            knows(&defense, 0, spell),
            "class {class} proficiency {spell}"
        );
    }
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn every_specialization_can_spend_thirteen_choices_with_effects() {
    let defense = defense_with_ranks("defense-supported-drafts");
    defense.start_team(0);
    let mut request = 3;
    for class in [1, 2, 3, 4, 5, 7, 8, 9, 11] {
        defense.node.assert_call(
            "wave_defense_fixture_reward_class",
            &[&defense.characters[0].guid, &class.to_string()],
        );
        for level in [1, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60] {
            set_level(&defense, 0, level, 0);
            let offer = defense.offer(0);
            for offered in offer[5..].chunks_exact(4) {
                let choice = offered[0].parse::<u32>().unwrap();
                let spell = if choice < 1_000_000 {
                    choice
                } else {
                    let talent = &defense.node.query_rows(&format!(
                        "SELECT spell_id, rank_spell_2, rank_spell_3, rank_spell_4, rank_spell_5, max_rank, grant_spell_id FROM game_talent WHERE talent_id = {}",
                        choice - 1_000_000,
                    ))[0];
                    let column = match talent["max_rank"].as_str() {
                        "1" => "spell_id",
                        "2" => "rank_spell_2",
                        "3" => "rank_spell_3",
                        "4" => "rank_spell_4",
                        "5" => "rank_spell_5",
                        rank => panic!("invalid rank {rank}"),
                    };
                    talent[column].parse::<u32>().unwrap()
                };
                let effects = defense.node.query_rows(&format!(
                    "SELECT kind, script_id FROM game_spell_effect WHERE spell_id = {spell}",
                ));
                assert!(
                    effects
                        .iter()
                        .any(|effect| !matches!(effect["kind"].as_str(), "15" | "190")
                            || effect["script_id"] != "0"),
                    "class {class} level {level} offered inert choice {choice}"
                );
            }
            defense.choose(0, request);
            request += 1;
        }
        assert_eq!(defense.member(0)["choices_spent"], "13", "class {class}");
    }
}
