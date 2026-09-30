//! Class decisions exercised through the Runner on private Standalone Shards.

mod support;

use std::time::Duration;
use support::Standalone;

fn fixture(class: &str, role: &str, grouped: bool, fighting: bool) -> (Standalone, String) {
    let mut node = Standalone::start("playerbots-class");
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    node.assert_call(
        "playerbots_spawn_class_role",
        &["2", "1200", "1200", "50", class, role],
    );
    let bots = node.query_rows("SELECT character_guid FROM pkg_playerbots_bot");
    let guid = bots[0]["character_guid"].clone();
    node.assert_call(
        "playerbots_fixture_class_stage",
        &[
            &guid,
            &bots[1]["character_guid"],
            &grouped.to_string(),
            &fighting.to_string(),
        ],
    );
    (node, guid)
}

fn pass(node: &Standalone, guid: &str) {
    node.assert_call("playerbots_fixture_runner_pass_once", &[guid]);
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_warrior_maintains_battle_shout_during_solo_and_party_combat() {
    for grouped in [false, true] {
        let (node, guid) = fixture("1", "0", grouped, true);
        node.assert_call("playerbots_fixture_class_shout_pass", &[&guid]);
        let auras = node.query_rows(&format!(
            "SELECT spell_id FROM game_aura WHERE target_guid = {guid}"
        ));
        assert!(
            auras.iter().any(|aura| aura["spell_id"] == "6673"),
            "Warrior did not apply Battle Shout, grouped={grouped}"
        );
        let aura_query = format!("SELECT id, applied_at FROM game_aura WHERE target_guid = {guid}");
        let before = node.query_rows(&aura_query);
        std::thread::sleep(Duration::from_millis(1600));
        pass(&node, &guid);
        assert_eq!(
            node.query_rows(&aura_query),
            before,
            "Battle Shout was refreshed while active"
        );
    }
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_starter_casters_apply_and_retain_their_buffs_solo_and_grouped() {
    for (class, role, spell) in [("5", "1", "1243"), ("8", "2", "168")] {
        for grouped in [false, true] {
            let (node, guid) = fixture(class, role, grouped, false);
            pass(&node, &guid);
            let query = format!(
                "SELECT id, spell_id, applied_at FROM game_aura WHERE target_guid = {guid}"
            );
            let before = node.query_rows(&query);
            assert!(
                before.iter().any(|aura| aura["spell_id"] == spell),
                "class {class} did not self-buff"
            );
            pass(&node, &guid);
            assert_eq!(
                node.query_rows(&query),
                before,
                "an existing buff was replaced"
            );
        }
    }
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_heroic_strike_fires_through_the_ordinary_melee_swing() {
    for grouped in [false, true] {
        let (node, guid) = fixture("1", "0", grouped, true);
        pass(&node, &guid);
        assert!(
            support::poll_until(Duration::from_secs(8), || {
                pass(&node, &guid);
                node.query_rows(&format!("SELECT spell_id, outcome FROM pkg_playerbots_action WHERE character_guid = {guid}"))
                .iter().any(|row| row["spell_id"] == "78" && row["outcome"].contains("castResolved"))
            }),
            "Heroic Strike was not queued after the buff cooldown"
        );
        assert!(
            support::poll_until(Duration::from_secs(8), || {
                node.query_rows(&format!("SELECT spell_id, is_interrupted, is_completion FROM game_spell_cast_event WHERE caster_guid = {guid}"))
                .iter().any(|event| event["spell_id"] == "78" && event["is_completion"] == "true" && event["is_interrupted"] == "false")
            }),
            "queued Heroic Strike never fired"
        );
        let me = &node.query_rows(&format!(
            "SELECT power, next_swing_spell FROM game_world_entity WHERE guid = {guid}"
        ))[0];
        assert_eq!(me["next_swing_spell"], "0");
        let strikes = node.query_rows(&format!("SELECT damage, target_guid, is_completion, is_interrupted FROM game_spell_cast_event WHERE caster_guid = {guid} AND spell_id = 78"));
        let strike = strikes
            .iter()
            .find(|event| event["is_completion"] == "true" && event["is_interrupted"] == "false")
            .expect("the completed strike disappeared");
        let target = &node.query_rows(&format!(
            "SELECT health FROM game_world_entity WHERE guid = {}",
            strike["target_guid"]
        ))[0];
        assert!(
            target["health"].parse::<u32>().unwrap()
                <= 10_000 - strike["damage"].parse::<u32>().unwrap()
        );
    }
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_queued_strike_keeps_its_rage_and_swing_timer() {
    let (node, guid) = fixture("1", "0", false, true);
    node.assert_call("playerbots_fixture_class_hold_queue", &[&guid]);
    let attack = format!(
        "SELECT target_guid, last_swing_ms FROM game_melee_attack WHERE attacker_guid = {guid}"
    );
    let before = node.query_rows(&attack);
    pass(&node, &guid);
    assert_eq!(node.query_rows(&attack), before);
    let me = &node.query_rows(&format!(
        "SELECT power, next_swing_spell FROM game_world_entity WHERE guid = {guid}"
    ))[0];
    // Incoming hits may add rage while the queued swing is held.
    assert!(me["power"].parse::<u32>().unwrap() >= 200);
    assert!(
        node.query_rows(&format!(
            "SELECT spell_id FROM game_aura WHERE target_guid = {guid} AND spell_id = 6673"
        ))
        .is_empty(),
        "Battle Shout spent rage reserved for Heroic Strike"
    );
    assert_eq!(me["next_swing_spell"], "78");
    assert!(
        node.query_rows(&format!(
            "SELECT spell_id FROM pkg_playerbots_action WHERE character_guid = {guid}"
        ))
        .iter()
        .all(|row| row["spell_id"] == "0"),
        "the queued strike was selected again"
    );
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_warrior_uses_the_shared_rotation_for_autonomous_grinding() {
    let (node, guid) = fixture("1", "0", false, false);
    let me = &node.query_rows(&format!(
        "SELECT x, y, z FROM game_world_entity WHERE guid = {guid}"
    ))[0];
    node.assert_sql(&format!(
        "UPDATE pkg_playerbots_bot SET home_x = {}, home_y = {}, home_z = {} WHERE character_guid = {guid}",
        me["x"], me["y"], me["z"]
    ));
    pass(&node, &guid);
    std::thread::sleep(Duration::from_millis(1600));
    pass(&node, &guid);
    let chosen = &node.query_rows(&format!(
        "SELECT chosen FROM pkg_playerbots_runner WHERE character_guid = {guid}"
    ))[0]["chosen"];
    assert!(
        chosen.contains("grind =") && chosen.contains("spell = 78"),
        "expected a grinding Heroic Strike: {chosen}"
    );
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_default_upgrade_preserves_operator_rotations() {
    let (node, _) = fixture("1", "0", false, false);
    for table in ["pkg_playerbots_rotation", "pkg_playerbots_kit"] {
        node.assert_sql(&format!("DELETE FROM {table} WHERE spell_id = 78"));
    }
    node.assert_sql("DELETE FROM game_package_config WHERE package_name = 'playerbots' AND key = 'class_defaults_revision'");
    node.assert_call(
        "playerbots_spawn_class_role",
        &["0", "1200", "1200", "50", "1", "0"],
    );
    assert_eq!(
        node.query_rows("SELECT spell_id FROM pkg_playerbots_rotation WHERE spell_id = 78")
            .len(),
        1
    );
    assert_eq!(
        node.query_rows("SELECT spell_id FROM pkg_playerbots_kit WHERE spell_id = 78")
            .len(),
        1
    );
    // An Operator may remove only Heroic Strike after the catalogue has been upgraded.
    for table in ["pkg_playerbots_rotation", "pkg_playerbots_kit"] {
        node.assert_sql(&format!("DELETE FROM {table} WHERE spell_id = 78"));
    }
    let mut before = node.query_rows("SELECT * FROM pkg_playerbots_rotation");
    before.sort();
    node.assert_call(
        "playerbots_spawn_class_role",
        &["0", "1200", "1200", "50", "1", "0"],
    );
    let mut after = node.query_rows("SELECT * FROM pkg_playerbots_rotation");
    after.sort();
    assert_eq!(after, before);
    assert!(node
        .query_rows("SELECT spell_id FROM pkg_playerbots_kit WHERE spell_id = 78")
        .is_empty());
    for table in ["pkg_playerbots_rotation", "pkg_playerbots_kit"] {
        node.assert_sql(&format!("DELETE FROM {table}"));
    }
    node.assert_call("playerbots_spawn", &["0", "1200", "1200", "50"]);
    for table in ["pkg_playerbots_rotation", "pkg_playerbots_kit"] {
        assert!(node
            .query_rows(&format!("SELECT * FROM {table}"))
            .is_empty());
    }
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_starter_casters_finish_attacks_solo_and_in_parties() {
    for (class, role, spell) in [("5", "1", "585"), ("8", "2", "133")] {
        for grouped in [false, true] {
            let (node, guid) = fixture(class, role, grouped, true);
            pass(&node, &guid);
            assert!(
                support::poll_until(Duration::from_secs(20), || {
                    pass(&node, &guid);
                    node.query_rows(&format!("SELECT spell_id, damage FROM game_spell_impact_event WHERE caster_guid = {guid}"))
                    .iter().any(|event| event["spell_id"] == spell && event["damage"].parse::<u32>().unwrap() > 0)
                }),
                "class {class} did not deal spell damage, grouped={grouped}; runner={:?}; actions={:?}; entity={:?}",
                node.query_rows(&format!("SELECT chosen, last_outcome, foreground, failures FROM pkg_playerbots_runner WHERE character_guid = {guid}")),
                node.query_rows(&format!("SELECT spell_id, outcome FROM pkg_playerbots_action WHERE character_guid = {guid}")),
                node.query_rows(&format!("SELECT health, power, x, y, combat_until_ms FROM game_world_entity WHERE guid = {guid}"))
            );
        }
    }
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_priest_completes_self_healing_solo_and_in_a_party() {
    for grouped in [false, true] {
        let (node, guid) = fixture("5", "1", grouped, false);
        node.assert_call("playerbots_fixture_companion_health", &[&guid, "25"]);
        let query = format!("SELECT health FROM game_world_entity WHERE guid = {guid}");
        let health = node.query_rows(&query)[0]["health"].parse::<u32>().unwrap();
        pass(&node, &guid);
        let casts = format!(
            "SELECT cast_id, spell_id FROM pkg_playerbots_action WHERE character_guid = {guid}"
        );
        let before = node.query_rows(&casts);
        assert!(before
            .iter()
            .any(|row| row["spell_id"] == "2050" && row["cast_id"] != "0"));
        pass(&node, &guid);
        assert_eq!(
            node.query_rows(&casts),
            before,
            "the second decision restarted the heal"
        );
        assert!(
            support::poll_until(Duration::from_secs(8), || {
                node.query_rows(&query)[0]["health"].parse::<u32>().unwrap() > health
            }),
            "Lesser Heal did not restore health, grouped={grouped}"
        );
    }
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_priest_heals_at_the_quest_safe_position_before_resuming_travel() {
    let mut node = Standalone::start("playerbots-priest-quest-recovery");
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    node.assert_call(
        "playerbots_spawn_class_role",
        &["1", "1200", "1200", "50", "5", "1"],
    );
    let guid = node.query_rows("SELECT character_guid FROM pkg_playerbots_bot")[0]
        ["character_guid"]
        .clone();
    node.assert_sql("DELETE FROM game_import_meta WHERE family = 'weather_seed'");
    node.assert_call("playerbots_quest_loop_fixture_stage_named", &[&guid]);
    node.assert_call("playerbots_quest_fixture_admit_accept", &[&guid, "7"]);
    node.assert_call("playerbots_fixture_position", &[&guid, "1340"]);
    node.assert_call("playerbots_fixture_runner_select_cohort", &[&guid]);
    pass(&node, &guid);
    let retained = node.query_rows(&format!(
        "SELECT safe_position FROM pkg_playerbots_quest_objective WHERE character_guid = {guid}"
    ));
    assert_eq!(retained.len(), 1, "{retained:?}");
    assert!(
        retained[0]["safe_position"].contains("x = 1340"),
        "{retained:?}"
    );
    node.assert_sql(&format!(
        "UPDATE pkg_playerbots_personality SET flee_at_pct = 30, heal_at_pct = 50 WHERE character_guid = {guid}"
    ));
    node.assert_call("playerbots_fixture_companion_health", &[&guid, "10"]);
    node.assert_sql(&format!(
        "DELETE FROM game_spell_cooldown WHERE caster_guid = {guid}"
    ));
    let health_query = format!("SELECT health FROM game_world_entity WHERE guid = {guid}");
    let before = node.query_rows(&health_query)[0]["health"]
        .parse::<u32>()
        .unwrap();
    pass(&node, &guid);
    let runner = node.query_rows(&format!(
        "SELECT chosen, objective FROM pkg_playerbots_runner WHERE character_guid = {guid}"
    ));
    assert!(runner[0]["objective"].contains("quest"), "{runner:?}");
    assert!(runner[0]["chosen"].contains("recovery"), "{runner:?}");
    assert!(node
        .query_rows(&format!(
            "SELECT spell_id, target_guid FROM game_pending_cast WHERE caster_guid = {guid}"
        ))
        .iter()
        .any(|cast| cast["spell_id"] == "2050" && cast["target_guid"] == guid));
    assert!(support::poll_until(Duration::from_secs(8), || {
        node.query_rows(&health_query)[0]["health"]
            .parse::<u32>()
            .unwrap()
            > before
    }));
}
