mod support;

use support::{Defense, Row};

fn replacements(defense: &Defense, guid: &str) -> Vec<Row> {
    defense.node.query_rows(&format!(
        "SELECT * FROM pkg_wave_defense_member WHERE replacement_for = {guid}"
    ))
}

fn reconnect_tick(defense: &Defense) {
    defense.node.assert_call(
        "gw_certify_package_party_authority",
        &["\"wave-defense-test\"", "true"],
    );
    defense
        .node
        .assert_call("wave_defense_fixture_reconnect_once", &[]);
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn a_disconnect_uses_a_separate_role_bot_and_reconnect_restores_the_human_slot() {
    let defense = Defense::start("defense-reconnect-slot");
    let run = defense.start_team(0);
    defense.choose(3, 3);
    let guid = &defense.characters[3].guid;
    let original = defense.member(3);
    let items = defense.node.query_rows(&format!(
        "SELECT * FROM game_item_instance WHERE owner_guid = {guid}"
    ));
    let spells = defense.node.query_rows(&format!(
        "SELECT * FROM game_player_spell WHERE character_guid = {guid}"
    ));
    defense
        .node
        .assert_call("wave_defense_fixture_disconnect", &[guid]);
    assert_eq!(
        defense.node.query_rows(&format!(
            "SELECT online FROM game_character WHERE guid = {guid}"
        ))[0]["online"],
        "false"
    );
    defense.advance(14);
    assert!(replacements(&defense, guid).is_empty());
    defense.node.assert_sql(&format!(
        "UPDATE pkg_wave_defense_match SET level = 30 WHERE id = {}",
        run["id"]
    ));
    defense.advance(2);
    let bots = replacements(&defense, guid);
    assert_eq!(bots.len(), 1);
    let bot = &bots[0]["character_guid"];
    assert_ne!(bot, guid);
    assert_eq!(bots[0]["class"], "8");
    let bot_character = defense.node.query_rows(&format!(
        "SELECT class, level, online FROM game_character WHERE guid = {bot}"
    ));
    assert_eq!(bot_character[0]["class"], "8");
    assert_eq!(bot_character[0]["level"], "30");
    assert_eq!(bot_character[0]["online"], "false");
    let roster = defense.node.query_rows(&format!(
        "SELECT character_guid FROM game_group_member WHERE group_id = {}",
        run["group_id"]
    ));
    assert_eq!(roster.len(), 5);
    assert!(!roster.iter().any(|row| row["character_guid"] == *guid));
    assert!(roster.iter().any(|row| row["character_guid"] == *bot));
    assert_eq!(
        defense.node.query_rows(&format!(
            "SELECT level FROM game_character WHERE guid = {guid}"
        ))[0]["level"],
        "1"
    );
    assert_eq!(
        defense.node.query_rows(&format!(
            "SELECT * FROM game_item_instance WHERE owner_guid = {guid}"
        )),
        items
    );
    assert_eq!(
        defense.node.query_rows(&format!(
            "SELECT * FROM game_player_spell WHERE character_guid = {guid}"
        )),
        spells
    );
    assert_eq!(defense.member(3)["acquired"], original["acquired"]);
    assert_eq!(
        defense.member(3)["choices_spent"],
        original["choices_spent"]
    );
    defense
        .node
        .assert_call("wave_defense_fixture_relogin", &[guid]);
    assert_eq!(defense.member(3)["resume_requested"], "true");
    assert_eq!(replacements(&defense, guid).len(), 1);
    reconnect_tick(&defense);
    assert!(replacements(&defense, guid).is_empty());
    assert!(defense
        .node
        .query_rows(&format!(
            "SELECT guid FROM game_character WHERE guid = {bot}"
        ))
        .is_empty());
    assert_eq!(
        defense.node.query_rows(&format!(
            "SELECT level, class FROM game_character WHERE guid = {guid}"
        ))[0]["level"],
        "30"
    );
    assert_eq!(defense.member(3)["class"], "4");
    assert_eq!(defense.member(3)["disconnected_micros"], "0");
    assert_eq!(defense.member(3)["resume_requested"], "false");
    assert_eq!(defense.member(3)["acquired"], original["acquired"]);
    assert_eq!(
        defense.member(3)["choices_spent"],
        original["choices_spent"]
    );
    assert_eq!(
        defense.node.query_rows(&format!(
            "SELECT * FROM game_item_instance WHERE owner_guid = {guid}"
        )),
        items
    );
    let restored = defense.node.query_rows(&format!(
        "SELECT character_guid FROM game_group_member WHERE group_id = {}",
        run["group_id"]
    ));
    assert_eq!(restored.len(), 5);
    assert!(restored.iter().any(|row| row["character_guid"] == *guid));
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn a_resume_refusal_rolls_back_the_party_swap_and_keeps_the_replacement() {
    let defense = Defense::start("defense-reconnect-atomic");
    let run = defense.start_team(0);
    let guid = &defense.characters[2].guid;
    defense
        .node
        .assert_call("wave_defense_fixture_disconnect", &[guid]);
    defense.advance(16);
    let bot = replacements(&defense, guid)[0]["character_guid"].clone();
    defense
        .node
        .assert_call("wave_defense_fixture_relogin", &[guid]);
    defense
        .node
        .assert_sql("DELETE FROM game_spell WHERE spell_id = 116");
    let before = defense.node.query_rows(&format!(
        "SELECT * FROM game_group_member WHERE group_id = {}",
        run["group_id"]
    ));
    let result = defense
        .node
        .call("wave_defense_fixture_reconnect_once", &[]);
    assert!(!result.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(text.contains("Spell is not imported"), "{text}");
    assert_eq!(
        defense.node.query_rows(&format!(
            "SELECT * FROM game_group_member WHERE group_id = {}",
            run["group_id"]
        )),
        before
    );
    assert_eq!(replacements(&defense, guid)[0]["character_guid"], bot);
    assert_eq!(defense.member(2)["resume_requested"], "true");
    assert_ne!(defense.member(2)["disconnected_micros"], "0");
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn an_explicit_departure_keeps_a_permanent_bot_and_returns_the_human_to_the_lobby() {
    let defense = Defense::start("defense-permanent-replacement");
    let run = defense.start_team(0);
    let guid = &defense.characters[3].guid;
    defense.command(3, "leave", &format!("3|{}", run["id"]));
    assert!(defense
        .node
        .query_rows(&format!(
            "SELECT * FROM pkg_wave_defense_member WHERE character_guid = {guid}"
        ))
        .is_empty());
    let bots = defense.node.query_rows(&format!(
        "SELECT * FROM pkg_wave_defense_member WHERE match_id = {} AND bot = true",
        run["id"]
    ));
    assert_eq!(bots.len(), 1);
    assert_eq!(bots[0]["replacement_for"], "0");
    assert_eq!(bots[0]["class"], "8");
    assert_eq!(
        defense
            .node
            .query_rows(&format!(
                "SELECT * FROM game_group_member WHERE group_id = {}",
                run["group_id"]
            ))
            .len(),
        5
    );
    let lobby = &defense
        .node
        .query_rows("SELECT lobby_instance_id FROM pkg_wave_defense_config")[0]
        ["lobby_instance_id"];
    let body = &defense.node.query_rows(&format!(
        "SELECT level, instance_id FROM game_world_entity WHERE guid = {guid}"
    ))[0];
    assert_eq!(body["level"], "1");
    assert_eq!(body["instance_id"], *lobby);
    defense.command(3, "select", "4|4");
    defense.command(3, "queue", "5");
    assert_eq!(
        defense
            .node
            .query_rows(&format!(
                "SELECT * FROM pkg_wave_defense_queue WHERE character_guid = {guid}"
            ))
            .len(),
        1
    );
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn manual_party_changes_and_release_spirit_cannot_break_a_match() {
    let defense = Defense::start("defense-manual-boundaries");
    let run = defense.start_team(0);
    let node = &defense.node;
    let before = node.query_rows(&format!(
        "SELECT * FROM game_group_member WHERE group_id = {}",
        run["group_id"]
    ));
    for operation in [0, 3, 4, 5, 6, 7] {
        let target = if operation == 0 {
            &defense.characters[5].guid
        } else {
            &defense.characters[1].guid
        };
        let result = node.call(
            "realm_group_op",
            &[
                &operation.to_string(),
                &defense.characters[0].actor,
                target,
                "0",
                "2",
                "0",
            ],
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            !result.status.success() && text.contains("group:action_suppressed"),
            "{operation}: {text}"
        );
    }
    assert_eq!(
        node.query_rows(&format!(
            "SELECT * FROM game_group_member WHERE group_id = {}",
            run["group_id"]
        )),
        before
    );
    node.assert_call("wave_defense_fixture_down", &[&defense.characters[3].guid]);
    let result = node.call("gw_repop", &[&defense.characters[3].actor]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !result.status.success() && text.contains("Wait for the match respawn or a resurrection"),
        "{text}"
    );
    defense.advance(0);
    defense.advance(16);
    assert_eq!(
        node.query_rows(&format!(
            "SELECT dead FROM game_world_entity WHERE guid = {}",
            defense.characters[3].guid
        ))[0]["dead"],
        "false"
    );
}
