mod support;

use support::{number, Defense, Row};

fn start(name: &str, level: u8) -> Defense {
    let mut defense = Defense::start(name);
    defense.characters[0].class = "11".into();
    let guid = &defense.characters[0].guid;
    let node = &defense.node;
    node.assert_sql(&format!(
        "UPDATE game_character SET class = 11, race = 4 WHERE guid = {guid}"
    ));
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET unit_bytes_0 = 2820 WHERE guid = {guid}"
    ));
    for level in 1..=60 {
        node.assert_sql(&format!(
            "DELETE FROM game_class_level_stats WHERE class_level = {}",
            (11 << 8) | level
        ));
        node.assert_sql(&format!("INSERT INTO game_class_level_stats (class_level,class,level,base_health,base_mana) VALUES ({},11,{level},1000,1000)", (11 << 8) | level));
        node.assert_sql(&format!(
            "DELETE FROM game_level_stats WHERE race_class_level = {}",
            (4 << 16) | (11 << 8) | level
        ));
        node.assert_sql(&format!("INSERT INTO game_level_stats (race_class_level,race,class,level,strength,agility,stamina,intellect,spirit) VALUES ({},4,11,{level},20,20,20,20,20)", (4 << 16) | (11 << 8) | level));
    }
    defense.start_team(0);
    node.assert_call(
        "wave_defense_fixture_reward_level",
        &[guid, &level.to_string(), "0"],
    );
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET simulation_indexed = false WHERE guid = {guid}"
    ));
    defense
}

fn body(defense: &Defense) -> Row {
    defense.node.query_rows(&format!(
        "SELECT * FROM game_world_entity WHERE guid = {}",
        defense.characters[0].guid
    ))[0]
        .clone()
}

fn cast(defense: &Defense, spell: &str, target: &str) {
    let guid = &defense.characters[0].guid;
    defense.node.assert_sql(&format!(
        "DELETE FROM game_spell_cooldown WHERE caster_guid = {guid}"
    ));
    defense
        .node
        .assert_call("debug_cast_at", &[guid, spell, target]);
}

fn saved_mana(defense: &Defense) -> Vec<Row> {
    defense.node.query_rows(&format!(
        "SELECT * FROM game_druid_form_mana WHERE character_guid = {}",
        defense.characters[0].guid
    ))
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn actual_bear_spells_generate_and_spend_rage_then_restore_mana() {
    let defense = start("defense-feral-rage", 20);
    let node = &defense.node;
    let guid = &defense.characters[0].guid;
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET health = 510, power = 900 WHERE guid = {guid}"
    ));
    cast(&defense, "5487", guid);
    let bear = body(&defense);
    assert_eq!(number::<u32>(&bear, "unit_bytes_0") >> 24, 1);
    assert_eq!(bear["max_power"], "1000");
    assert_eq!(bear["power"], "0");
    assert_eq!(bear["display_id"], "2281");
    assert_eq!(bear["max_health"], "1220");
    assert_eq!(bear["health"], "610");
    assert_eq!(saved_mana(&defense)[0]["mana"], "350");
    node.assert_call("debug_apply_damage", &[guid, "200", "0"]);
    assert_eq!(body(&defense)["power"], "600");
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET godmode = true WHERE guid = {guid}"
    ));
    node.assert_call("debug_spawn_at_feet", &[guid, "5097000", "2"]);
    let target = node.query_rows("SELECT guid FROM game_world_entity WHERE entry = 5097000")[0]
        ["guid"]
        .clone();
    node.assert_sql(&format!("UPDATE game_world_entity SET simulation_indexed = false, health = 10000, max_health = 10000, level = 20 WHERE guid = {target}"));
    cast(&defense, "6807", &target);
    assert_eq!(body(&defense)["power"], "450");
    cast(&defense, "779", &target);
    assert_eq!(body(&defense)["power"], "250");
    node.assert_sql(&format!(
        "DELETE FROM game_spell_cooldown WHERE caster_guid = {guid}"
    ));
    let mana_refused = node.call("debug_cast_at", &[guid, "5185", guid]);
    assert!(!mana_refused.status.success());
    assert!(String::from_utf8_lossy(&mana_refused.stderr).contains("different form resource"));
    assert_eq!(body(&defense)["power"], "250");
    assert_eq!(saved_mana(&defense)[0]["mana"], "350");
    cast(&defense, "5487", guid);
    let caster = body(&defense);
    assert_eq!(number::<u32>(&caster, "unit_bytes_0") >> 24, 0);
    assert_eq!(caster["stance"], "0");
    assert_eq!(caster["max_power"], "1020");
    assert_eq!(caster["power"], "350");
    assert_eq!(caster["max_health"], "1020");
    assert_eq!(caster["health"], "342");
    assert!(saved_mana(&defense).is_empty());
    node.assert_sql(&format!(
        "DELETE FROM game_spell_cooldown WHERE caster_guid = {guid}"
    ));
    let before = body(&defense);
    let refused = node.call("debug_cast_at", &[guid, "5487", guid]);
    assert!(
        !refused.status.success(),
        "Bear Form ignored the base-mana cost"
    );
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not enough power"));
    assert_eq!(body(&defense), before);
    assert!(saved_mana(&defense).is_empty());
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn match_leveling_preserves_bear_rage() {
    let defense = start("defense-feral-level", 20);
    let guid = &defense.characters[0].guid;
    cast(&defense, "5487", guid);
    defense
        .node
        .assert_call("debug_apply_damage", &[guid, "100", "0"]);
    assert_eq!(body(&defense)["power"], "300");
    defense
        .node
        .assert_call("wave_defense_fixture_reward_level", &[guid, "25", "0"]);
    assert_eq!(body(&defense)["power"], "300");
    assert_eq!(body(&defense)["max_power"], "1000");
    assert_eq!(saved_mana(&defense)[0]["mana"], "470");
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn actual_bear_passives_scale_equipment_armor_and_health_at_their_levels() {
    let defense = start("defense-feral-stats", 20);
    let node = &defense.node;
    let guid = &defense.characters[0].guid;
    node.assert_call("debug_grant_item", &[guid, "5090050", "1"]);
    node.assert_sql(
        "UPDATE game_item_template SET stat_armor = 100, max_durability = 0 WHERE entry = 5090050",
    );
    node.assert_sql(&format!(
        "UPDATE game_item_instance SET slot = 4 WHERE owner_guid = {guid} AND entry = 5090050"
    ));
    let attacker = &defense.characters[1].guid;
    node.assert_sql(&format!("UPDATE game_world_entity SET level = 20, simulation_indexed = false WHERE guid = {attacker}"));
    cast(&defense, "5487", guid);
    node.assert_call("debug_compute_swing", &[attacker, guid]);
    let swing = &node.query_rows("SELECT * FROM game_debug_readout WHERE key = 'swing'")[0];
    assert_eq!(swing["mitigation_pct"], "13");
    assert_eq!(body(&defense)["sheet_ap_mods"], "60");
    assert_eq!(body(&defense)["max_health"], "1220");
    assert_eq!(body(&defense)["stamina"], "20");
    cast(&defense, "5487", guid);
    node.assert_call("wave_defense_fixture_reward_level", &[guid, "60", "0"]);
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET power = 1020 WHERE guid = {guid}"
    ));
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET level = 60 WHERE guid = {attacker}"
    ));
    cast(&defense, "9634", guid);
    node.assert_call("debug_compute_swing", &[attacker, guid]);
    assert_eq!(
        node.query_rows("SELECT mitigation_pct FROM game_debug_readout WHERE key = 'swing'")[0]
            ["mitigation_pct"],
        "8"
    );
    assert_eq!(body(&defense)["sheet_ap_mods"], "180");
    assert_eq!(body(&defense)["max_health"], "2260");
    assert_eq!(body(&defense)["stamina"], "20");
    assert_eq!(body(&defense)["max_power"], "1000");
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn leaving_a_match_clears_form_and_saved_mana() {
    let defense = start("defense-feral-reset", 60);
    let guid = &defense.characters[0].guid;
    cast(&defense, "9634", guid);
    assert_eq!(body(&defense)["stance"], "5");
    assert_eq!(saved_mana(&defense).len(), 1);
    let run = defense.run(0);
    defense.command(0, "leave", &format!("4|{}", run["id"]));
    let reset = body(&defense);
    assert_eq!(reset["stance"], "0");
    assert_eq!(number::<u32>(&reset, "unit_bytes_0") >> 24, 0);
    assert_eq!(reset["unit_bytes_1"], "0");
    assert_eq!(reset["level"], "1");
    assert_eq!(reset["max_health"], "1020");
    assert_eq!(reset["power"], "1020");
    assert_eq!(reset["display_id"], reset["native_display_id"]);
    assert!(saved_mana(&defense).is_empty());
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn wave_clear_refills_mana_while_still_in_bear_form() {
    let defense = start("defense-feral-refill", 20);
    let guid = &defense.characters[0].guid;
    cast(&defense, "5487", guid);
    assert_eq!(saved_mana(&defense)[0]["mana"], "470");
    let run = defense.run(0);
    defense.advance(60);
    defense.clear(&run["id"]);
    assert_eq!(body(&defense)["stance"], "3");
    assert_eq!(body(&defense)["power"], "0");
    assert_eq!(saved_mana(&defense)[0]["mana"], "1020");
    cast(&defense, "5487", guid);
    assert_eq!(body(&defense)["power"], "1020");
}

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn login_restores_caster_mana_from_a_shifted_body() {
    let defense = start("defense-feral-login", 20);
    let node = &defense.node;
    let character = &defense.characters[0];
    cast(&defense, "5487", &character.guid);
    node.assert_call("debug_apply_damage", &[&character.guid, "100", "0"]);
    assert_eq!(body(&defense)["power"], "300");
    let key = serde_json::to_string(&vec![7u8; 40]).unwrap();
    node.assert_call(
        "establish_session",
        &[&character.account, &key, r#"{"__identity__":"0x1"}"#],
    );
    node.assert_call("gw_heartbeat", &[]);
    node.assert_call("gw_player_login", &[&character.account, &character.actor]);
    let restored = body(&defense);
    assert_eq!(restored["stance"], "0");
    assert_eq!(number::<u32>(&restored, "unit_bytes_0") >> 24, 0);
    assert_eq!(restored["power"], "470");
    assert_eq!(restored["max_power"], "1020");
    assert!(saved_mana(&defense).is_empty());
}
