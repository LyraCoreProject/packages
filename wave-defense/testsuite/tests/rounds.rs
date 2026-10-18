mod support;

use support::Defense;

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn all_ready_starts_early_and_an_idle_member_cannot_extend_preparation() {
    let defense = Defense::start("defense-ready");
    let run = defense.start_team(0);
    for index in 0..5 {
        defense.command(index, "ready", &format!("3|{}|1", run["id"]));
    }
    defense.advance(0);
    assert_eq!(defense.phase(0), "combat");
    defense.clear(&run["id"]);
    assert_eq!(defense.phase(0), "preparation");
    defense.refused(
        4,
        "ready",
        &format!("4|{}|1", run["id"]),
        "This preparation has ended",
    );
    for index in 0..4 {
        defense.command(index, "ready", &format!("4|{}|2", run["id"]));
    }
    defense.advance(5);
    assert_eq!(defense.phase(0), "preparation");
    assert_eq!(defense.member(4)["ready"], "false");
    defense.advance(20);
    assert_eq!(defense.phase(0), "combat");
    assert_eq!(defense.run(0)["wave"], "2");
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn fifteen_waves_grant_thirteen_choices_and_progress_without_permanent_match_power() {
    let defense = Defense::start("defense-full-round");
    let node = &defense.node;
    let initial = defense.start_team(0);
    let levels = [1, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 60, 60];
    let earned_choices = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 13, 13];
    let mut spent = 0;
    let mut request = 3;
    for index in 0..15 {
        let run = defense.run(0);
        assert_eq!(run["wave"].parse::<usize>().unwrap(), index + 1);
        assert_eq!(run["level"].parse::<u8>().unwrap(), levels[index]);
        assert_eq!(defense.phase(0), "preparation");
        while spent < earned_choices[index] {
            defense.choose(2, request);
            spent += 1;
            request += 1;
        }
        assert_eq!(
            defense.member(2)["choices_spent"].parse::<u8>().unwrap(),
            earned_choices[index]
        );
        defense.advance(31);
        defense.clear(&initial["id"]);
        defense.advance(0);
    }
    assert_eq!(spent, 13);
    let won = defense.run(0);
    assert_eq!(defense.phase(0), "won");
    assert_eq!(won["level"], "60");
    for index in 0..5 {
        let progress = &node.query_rows(&format!(
            "SELECT waves, bosses, wins FROM pkg_wave_defense_progress WHERE account_id = {}",
            defense.characters[index].account,
        ))[0];
        assert_eq!(progress["waves"], "15");
        assert_eq!(progress["bosses"], "3");
        assert_eq!(progress["wins"], "1");
        let body = &node.query_rows(&format!(
            "SELECT level, money FROM game_world_entity WHERE guid = {}",
            defense.characters[index].guid,
        ))[0];
        assert_eq!(body["level"], "60");
        assert_eq!(body["money"], "750000");
    }
    node.assert_call(
        "debug_grant_item",
        &[&defense.characters[2].guid, "5090050", "1"],
    );
    let match_items = node.query_rows(&format!(
        "SELECT guid FROM game_item_instance WHERE owner_guid = {}",
        defense.characters[2].guid,
    ));
    defense.advance(31);
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_match")
        .is_empty());
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_member")
        .is_empty());
    assert!(node
        .query_rows("SELECT * FROM game_package_combat_rule")
        .is_empty());
    let lobby_instance = node.query_rows("SELECT lobby_instance_id FROM pkg_wave_defense_config")
        [0]["lobby_instance_id"]
        .clone();
    for index in 0..5 {
        let guid = &defense.characters[index].guid;
        let body = &node.query_rows(&format!(
            "SELECT level, money, instance_id FROM game_world_entity WHERE guid = {guid}",
        ))[0];
        assert_eq!(body["level"], "1");
        assert_eq!(body["money"], "0");
        assert_eq!(body["instance_id"], lobby_instance);
        let character = &node.query_rows(&format!(
            "SELECT level, money FROM game_character WHERE guid = {guid}",
        ))[0];
        assert_eq!(character["level"], "1");
        assert_eq!(character["money"], "0");
    }
    assert!(node
        .query_rows(&format!(
            "SELECT * FROM game_character_talent WHERE character_guid = {}",
            defense.characters[2].guid,
        ))
        .is_empty());
    assert!(node
        .query_rows(&format!(
            "SELECT * FROM game_player_spell WHERE character_guid = {} AND spell_id = 116",
            defense.characters[2].guid,
        ))
        .is_empty());
    for item in match_items {
        assert!(node
            .query_rows(&format!(
                "SELECT * FROM game_item_instance WHERE guid = {}",
                item["guid"]
            ))
            .is_empty());
    }
    let status = defense.offer(2);
    assert_eq!(status, vec!["v1", "0", "0", "0", "0"]);
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn dead_members_share_levels_and_base_destruction_retains_completed_wave_progress() {
    let defense = Defense::start("defense-loss");
    let node = &defense.node;
    let run = defense.start_team(0);
    defense.advance(31);
    node.assert_call("wave_defense_fixture_down", &[&defense.characters[4].guid]);
    defense.advance(0);
    defense.clear(&run["id"]);
    let body = &node.query_rows(&format!(
        "SELECT dead, level FROM game_world_entity WHERE guid = {}",
        defense.characters[4].guid,
    ))[0];
    assert_eq!(body["dead"], "true");
    assert_eq!(body["level"], "5");
    assert_eq!(defense.offer(4)[4], "2");
    defense.advance(31);
    defense.advance(0);
    node.assert_call("wave_defense_fixture_pressure", &[&run["id"]]);
    let lost = defense.run(0);
    assert_eq!(defense.phase(0), "lost");
    assert_eq!(lost["base_health"], "0");
    for index in 0..5 {
        let progress = &node.query_rows(&format!(
            "SELECT waves, bosses, wins FROM pkg_wave_defense_progress WHERE account_id = {}",
            defense.characters[index].account,
        ))[0];
        assert_eq!(progress["waves"], "1");
        assert_eq!(progress["bosses"], "0");
        assert_eq!(progress["wins"], "0");
    }
    defense.advance(31);
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_match")
        .is_empty());
}
