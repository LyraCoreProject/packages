mod support;

use support::Defense;

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn authenticated_commands_refuse_forgery_and_roll_back_invalid_intent() {
    let defense = Defense::start("defense-commands");
    let node = &defense.node;
    let forged = defense.characters[0]
        .actor
        .replace("\"generation\":1", "\"generation\":2");
    defense.refused_actor(&forged, "select", "1|1", "STALE_WORLD_SESSION");
    let other_character = defense.characters[0].actor.replace(
        &format!("\"guid\":{}", defense.characters[0].guid),
        &format!("\"guid\":{}", defense.characters[1].guid),
    );
    defense.refused_actor(&other_character, "select", "1|5", "STALE_WORLD_SESSION");
    defense.refused_actor(
        &format!(
            r#"{{"guid":{},"ownership":null}}"#,
            defense.characters[0].guid
        ),
        "status",
        "",
        "STALE_WORLD_SESSION",
    );
    defense.refused(0, "select", "1|8", "this Character");
    defense.refused(0, "select", "1|1|unexpected", "Too many");
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_profile")
        .is_empty());
    defense.refused(0, "status", "another Actor", "no payload");
    defense.select(0);
    defense.refused(0, "queue", "0", "Stale request");
    defense.queue(0);
    defense.queue(0);
    assert_eq!(
        node.query_rows("SELECT * FROM pkg_wave_defense_queue")
            .len(),
        1
    );
    assert_eq!(
        node.query_rows("SELECT last_request FROM pkg_wave_defense_profile")[0]["last_request"],
        "2"
    );
    defense.refused(0, "select", "1|1", "Stale request");
    let owner = node.query_rows(&format!(
        "SELECT owner_identity FROM game_character WHERE guid = {}",
        defense.characters[0].guid,
    ))[0]["owner_identity"]
        .clone();
    for reply in node.query_rows("SELECT recipient_identity, cmd FROM game_addon_message") {
        assert_eq!(reply["recipient_identity"], owner);
        assert!(reply["cmd"].starts_with("package.wave_defense."));
    }
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn premades_need_each_humans_consent_and_two_matches_keep_separate_instances() {
    let defense = Defense::start("defense-consent");
    let node = &defense.node;
    let premade = format!(
        "[{},{}]",
        defense.characters[0].guid, defense.characters[2].guid
    );
    node.assert_call("debug_package_form_party", &[&premade]);
    let group = node.query_rows(&format!(
        "SELECT group_id FROM game_group_member WHERE character_guid = {}",
        defense.characters[0].guid,
    ))[0]["group_id"]
        .clone();
    for index in 0..5 {
        defense.select(index);
    }
    for index in [0, 1, 3, 4] {
        defense.queue(index);
    }
    defense.advance(0);
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_match")
        .is_empty());
    defense.queue(2);
    defense.advance(0);
    let first = defense.run(0);
    assert_eq!(first["group_id"], group);
    assert_eq!(defense.run(2)["id"], first["id"]);
    let second = defense.start_team(5);
    assert_ne!(first["instance_id"], second["instance_id"]);
    assert_ne!(first["id"], second["id"]);
    assert_eq!(
        node.query_rows("SELECT * FROM pkg_wave_defense_match")
            .len(),
        2
    );
    assert_eq!(
        node.query_rows("SELECT * FROM pkg_wave_defense_member")
            .len(),
        10
    );
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_queue")
        .is_empty());
    for (index, instance) in [
        (0, &first["instance_id"]),
        (4, &first["instance_id"]),
        (5, &second["instance_id"]),
        (9, &second["instance_id"]),
    ] {
        let body = &node.query_rows(&format!(
            "SELECT instance_id, map_id FROM game_world_entity WHERE guid = {}",
            defense.characters[index].guid,
        ))[0];
        assert_eq!(body["instance_id"], *instance);
        assert_eq!(body["map_id"], "189");
    }
    defense.command(0, "ready", &format!("3|{}|1", first["id"]));
    assert_eq!(defense.member(0)["ready"], "true");
    assert_eq!(defense.member(5)["ready"], "false");
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn draft_replay_stale_revision_and_combat_choice_do_not_grant_twice() {
    let defense = Defense::start("defense-draft");
    let node = &defense.node;
    let run = defense.start_team(0);
    let offer = defense.choose(2, 3);
    let before = node.query_rows(&format!(
        "SELECT * FROM game_player_spell WHERE character_guid = {}",
        defense.characters[2].guid,
    ));
    let talents = node.query_rows(&format!(
        "SELECT * FROM game_character_talent WHERE character_guid = {}",
        defense.characters[2].guid,
    ));
    let payload = format!("3|{}|{}|{}", offer[1], offer[2], offer[5]);
    defense.command(2, "choose", &payload);
    assert_eq!(defense.member(2)["choices_spent"], "1");
    assert_eq!(
        node.query_rows(&format!(
            "SELECT * FROM game_player_spell WHERE character_guid = {}",
            defense.characters[2].guid,
        )),
        before
    );
    assert_eq!(
        node.query_rows(&format!(
            "SELECT * FROM game_character_talent WHERE character_guid = {}",
            defense.characters[2].guid,
        )),
        talents
    );
    defense.refused(
        2,
        "choose",
        &payload.replacen("3|", "4|", 1),
        "no longer available",
    );
    defense.advance(31);
    let banked = defense.offer(0);
    defense.refused(
        0,
        "choose",
        &format!("3|{}|{}|{}", banked[1], banked[2], banked[5]),
        "no longer available",
    );
    defense.clear(&run["id"]);
    let after = defense.offer(0);
    assert_eq!(after[4], "2");
    assert_eq!(defense.member(0)["choices_spent"], "0");
    defense.choose(0, 3);
    defense.choose(0, 4);
    assert_eq!(defense.member(0)["choices_spent"], "2");
}
