mod support;

use support::{poll_until, Standalone, POLL_TIMEOUT};

fn stage(case: &str, grouped: bool) -> Standalone {
    let mut node = Standalone::start(case);
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["15000000"]);
    node.assert_call(
        "gw_certify_package_party_authority",
        &["\"wave-defense-test\"", "true"],
    );
    node.assert_call(
        "playerbots_fixture_wave_defense_stage",
        &[if grouped { "true" } else { "false" }],
    );
    node
}

fn role(node: &Standalone, role: u8) -> String {
    node.query_rows(&format!(
        "SELECT character_guid FROM pkg_playerbots_bot WHERE role = {role}"
    ))[0]["character_guid"]
        .clone()
}

fn pass(node: &Standalone, guid: &str) {
    node.assert_call("playerbots_fixture_runner_pass_once", &[guid]);
}

#[test]
#[ignore = "requires private SpacetimeDB and the playerbots Package"]
fn wave_defense_bots_are_sessionless_characters_with_role_spells() {
    let node = stage("wave-bot-roster", true);
    assert!(node.query_rows("SELECT * FROM game_session").is_empty());
    assert_eq!(
        node.query_rows("SELECT * FROM pkg_playerbots_wave_defense")
            .len(),
        3
    );
    assert_eq!(node.query_rows("SELECT * FROM game_group_member").len(), 3);
    for (role_id, spell, name) in [
        (0, 355, "Tankbot"),
        (1, 2050, "Healbot"),
        (2, 133, "Dpsbot"),
    ] {
        let guid = role(&node, role_id);
        let character = &node.query_rows(&format!(
            "SELECT name, level, online FROM game_character WHERE guid = {guid}"
        ))[0];
        assert!(character["name"].contains(name));
        assert_eq!(character["level"], "5");
        assert_eq!(character["online"], "false");
        assert_eq!(node.query_rows(&format!("SELECT * FROM game_player_spell WHERE character_guid = {guid} AND spell_id = {spell}")).len(), 1);
    }
}

#[test]
#[ignore = "requires private SpacetimeDB and the playerbots Package"]
fn wave_defense_tank_engages_before_any_party_member_pulls() {
    let node = stage("wave-bot-engage", true);
    let tank = role(&node, 0);
    node.assert_sql("UPDATE game_creature_template SET aggro_range = 0 WHERE entry = 51000");
    node.assert_call("debug_spawn_at_feet", &[&tank, "51000", "2"]);
    let target = node
        .query_rows("SELECT guid FROM game_world_entity WHERE entry = 51000 AND x > 1190")[0]
        ["guid"]
        .clone();
    assert!(node.query_rows("SELECT * FROM game_threat").is_empty());
    node.assert_call(
        "playerbots_fixture_wave_defense_targets",
        &[&tank, &format!("[{target}]")],
    );
    assert!(
        poll_until(POLL_TIMEOUT, || {
            pass(&node, &tank);
            !node.query_rows(&format!("SELECT * FROM game_melee_attack WHERE attacker_guid = {tank} AND target_guid = {target}")).is_empty()
        }),
        "tank never engaged the wave target: runner={:?}, target={:?}, actions={:?}",
        node.query_rows(&format!("SELECT * FROM pkg_playerbots_runner WHERE character_guid = {tank}")),
        node.query_rows(&format!("SELECT guid, health, faction_template, x, y, z FROM game_world_entity WHERE guid = {target}")),
        node.query_rows(&format!("SELECT * FROM pkg_playerbots_action WHERE character_guid = {tank}"))
    );
}

#[test]
#[ignore = "requires private SpacetimeDB and the playerbots Package"]
fn wave_defense_healer_heals_a_bot_led_party() {
    let node = stage("wave-bot-heal", true);
    let tank = role(&node, 0);
    let healer = role(&node, 1);
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET health = 1 WHERE guid = {tank}"
    ));
    assert!(
        poll_until(POLL_TIMEOUT, || {
            pass(&node, &healer);
            node.query_rows(&format!(
                "SELECT health FROM game_world_entity WHERE guid = {tank}"
            ))[0]["health"]
                .parse::<u32>()
                .unwrap()
                > 1
        }),
        "healer did not heal its Party member"
    );
}

#[test]
#[ignore = "requires private SpacetimeDB and the playerbots Package"]
fn wave_defense_lifecycle_changes_only_the_owned_bot() {
    let node = stage("wave-bot-lifecycle", true);
    let tank = role(&node, 0);
    let healer = role(&node, 1);
    node.assert_call("playerbots_fixture_wave_defense_level", &[&tank, "60"]);
    assert_eq!(
        node.query_rows(&format!(
            "SELECT level FROM game_character WHERE guid = {tank}"
        ))[0]["level"],
        "60"
    );
    assert_eq!(
        node.query_rows(&format!(
            "SELECT level FROM game_world_entity WHERE guid = {tank}"
        ))[0]["level"],
        "60"
    );
    node.assert_call("playerbots_fixture_wave_defense_remove", &[&tank]);
    for table in [
        "pkg_playerbots_wave_defense",
        "pkg_playerbots_bot",
        "game_group_member",
    ] {
        assert!(node
            .query_rows(&format!(
                "SELECT * FROM {table} WHERE character_guid = {tank}"
            ))
            .is_empty());
    }
    assert!(node
        .query_rows(&format!("SELECT * FROM game_character WHERE guid = {tank}"))
        .is_empty());
    assert_eq!(
        node.query_rows(&format!(
            "SELECT * FROM game_character WHERE guid = {healer}"
        ))
        .len(),
        1
    );
}

#[test]
#[ignore = "requires private SpacetimeDB and the playerbots Package"]
fn wave_defense_bots_do_not_grind_before_the_party_forms() {
    let node = stage("wave-bot-wait", false);
    let tank = role(&node, 0);
    node.assert_sql("UPDATE game_creature_template SET aggro_range = 0 WHERE entry = 51000");
    node.assert_call("debug_spawn_at_feet", &[&tank, "51000", "2"]);
    for _ in 0..5 {
        pass(&node, &tank);
        assert!(node
            .query_rows(&format!(
                "SELECT * FROM game_melee_attack WHERE attacker_guid = {tank}"
            ))
            .is_empty());
    }
    let runner = &node.query_rows(&format!(
        "SELECT chosen FROM pkg_playerbots_runner WHERE character_guid = {tank}"
    ))[0];
    assert!(!runner["chosen"].contains("grind"));
    assert!(node
        .query_rows(&format!(
            "SELECT * FROM pkg_playerbots_quest_objective WHERE character_guid = {tank}"
        ))
        .is_empty());
}

#[test]
#[ignore = "requires private SpacetimeDB and the playerbots Package"]
fn wave_defense_bots_wait_for_match_respawn() {
    let node = stage("wave-bot-respawn", true);
    let tank = role(&node, 0);
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET dead = true, health = 0 WHERE guid = {tank}"
    ));
    pass(&node, &tank);
    let body = &node.query_rows(&format!(
        "SELECT dead, map_id, instance_id, x, y FROM game_world_entity WHERE guid = {tank}"
    ))[0];
    assert_eq!(body["dead"], "true");
    assert_eq!(body["x"].parse::<f32>().unwrap(), 1200.0);
    assert_eq!(body["y"].parse::<f32>().unwrap(), 1200.0);
    let runner = &node.query_rows(&format!(
        "SELECT chosen FROM pkg_playerbots_runner WHERE character_guid = {tank}"
    ))[0];
    assert!(runner["chosen"].contains("hold"));
}
