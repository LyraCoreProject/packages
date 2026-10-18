mod support;

use support::{number, poll_until, Defense, Row, POLL_TIMEOUT};

#[test]
#[ignore = "requires private SpacetimeDB, Wasm and Operator-supplied client assets"]
fn actual_hunter_spells_recall_dismissed_pets_and_revive_dead_pets_in_combat() {
    let defense = Defense::start("defense-hunter-spells");
    defense.start_team(0);
    let node = &defense.node;
    let hunter = &defense.characters[4].guid;
    let pet = || -> Row {
        node.query_rows(&format!(
            "SELECT * FROM game_world_entity WHERE owner_guid = {hunter}"
        ))[0]
            .clone()
    };
    let initial = pet();
    let identity = node.query_rows(&format!(
        "SELECT pet_id, name, pet_xp FROM game_hunter_pet WHERE owner_guid = {hunter}"
    ));
    let cast = |spell: &str| {
        node.assert_sql(&format!(
            "DELETE FROM game_spell_cooldown WHERE caster_guid = {hunter}"
        ));
        node.assert_sql(&format!(
            "UPDATE game_world_entity SET power = 10000, max_power = 10000 WHERE guid = {hunter}"
        ));
        node.assert_call("debug_cast_at", &[hunter, spell, hunter]);
    };
    cast("2641");
    assert!(node
        .query_rows(&format!(
            "SELECT * FROM game_world_entity WHERE owner_guid = {hunter}"
        ))
        .is_empty());
    cast("883");
    for field in [
        "guid",
        "entry",
        "level",
        "max_health",
        "health",
        "sheet_dmg_min",
        "sheet_dmg_max",
    ] {
        assert_eq!(pet()[field], initial[field], "recall changed {field}");
    }
    assert_eq!(
        identity,
        node.query_rows(&format!(
            "SELECT pet_id, name, pet_xp FROM game_hunter_pet WHERE owner_guid = {hunter}"
        ))
    );
    node.assert_call("debug_set_health", &[&initial["guid"], "0"]);
    node.assert_sql(&format!(
        "DELETE FROM game_spell_cooldown WHERE caster_guid = {hunter}"
    ));
    let refused = node.call("debug_cast_at", &[hunter, "883", hunter]);
    assert!(!refused.status.success(), "Call Pet revived a dead pet");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Revive the dead pet"));
    node.assert_sql(&format!("UPDATE game_world_entity SET power = 10000, max_power = 10000, unit_flags = 524288 WHERE guid = {hunter}"));
    node.assert_call("debug_begin_cast", &[hunter, "982", hunter]);
    assert!(
        poll_until(POLL_TIMEOUT, || {
            let rows = node.query_rows(&format!(
                "SELECT dead FROM game_world_entity WHERE owner_guid = {hunter}"
            ));
            rows.first().is_some_and(|row| row["dead"] == "false")
        }),
        "actual Revive Pet did not complete"
    );
    let revived = pet();
    assert!(number::<u32>(&revived, "health") > 0);
    assert!(number::<u32>(&revived, "health") < number::<u32>(&revived, "max_health"));
    assert_eq!(
        identity,
        node.query_rows(&format!(
            "SELECT pet_id, name, pet_xp FROM game_hunter_pet WHERE owner_guid = {hunter}"
        ))
    );
    assert_eq!(
        node.query_rows(&format!(
            "SELECT dead FROM game_hunter_pet WHERE owner_guid = {hunter}"
        ))[0]["dead"],
        "false"
    );
}
