//! Spell progression through Provisioning and the ordinary Runner on a private Shard.

mod support;

use std::collections::BTreeMap;
use std::time::Duration;
use support::Standalone;

fn insert_row(node: &Standalone, table: &str, row: BTreeMap<String, String>) {
    let columns = match table {
        "game_spell" => "spell_id,name,power_type,cost,cast_time_ms,gcd_ms,cooldown_ms,range_yd,duration_ms,school_mask,dispel_type,mechanic,max_stacks,aura_interrupt,attributes,spell_level,max_level,is_negative,cast_flags,stances,family_name,family_flags,proc_flags,proc_chance,proc_charges",
        "game_spell_effect" => "id,spell_id,effect_index,kind,base_points,die_sides,per_level,period_ms,target,radius_yd,chain_targets,trigger_spell,effect_mechanic,p_0,p_0_kind,p_1,script_id,enters_combat",
        _ => panic!("unsupported fixture table"),
    };
    let values = columns
        .split(',')
        .map(|key| {
            let value = &row[key];
            if key == "name" {
                format!("'{}'", value.trim_matches('"').replace('\'', "''"))
            } else {
                value.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    node.assert_sql(&format!(
        "INSERT INTO {table} ({columns}) VALUES ({values})"
    ));
}

fn verify_ranks(class: &str, role: &str, ranks: &[(u32, u32, u8, u8)], expected: u32, future: u32) {
    let first = ranks[0].0;
    let mut node = Standalone::start("playerbots-spell-ranks");
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    node.assert_call(
        "playerbots_spawn_class_role",
        &["2", "1200", "1200", "50", class, role],
    );
    let bots = node.query_rows("SELECT character_guid FROM pkg_playerbots_bot");
    let guid = &bots[0]["character_guid"];
    node.assert_call(
        "playerbots_fixture_class_stage",
        &[guid, &bots[1]["character_guid"], "false", "false"],
    );
    node.assert_call("debug_set_level", &[guid, "20"]);
    let base = node.query_rows(&format!(
        "SELECT * FROM game_spell WHERE spell_id = {first}"
    ))[0]
        .clone();
    let effect = node.query_rows(&format!(
        "SELECT * FROM game_spell_effect WHERE spell_id = {first}"
    ))[0]
        .clone();
    for &(spell, previous, rank, level) in ranks {
        if spell != first {
            let mut rank = base.clone();
            rank.insert("spell_id".into(), spell.to_string());
            rank.insert("spell_level".into(), level.to_string());
            insert_row(&node, "game_spell", rank);
            let mut effect = effect.clone();
            effect.insert("id".into(), (u64::from(spell) << 2).to_string());
            effect.insert("spell_id".into(), spell.to_string());
            insert_row(&node, "game_spell_effect", effect);
        }
        node.assert_sql(&format!("INSERT INTO game_spell_chain (spell_id,prev_spell,first_spell,rank,req_spell) VALUES ({spell},{previous},{first},{rank},0)"));
        node.assert_sql(&format!("INSERT INTO game_trainer_spell (id,trainer_entry,spell_id,cost,required_level,learn_skill_line,learn_skill_cap) VALUES ({spell},51001,{spell},100,{level},0,75)"));
    }
    node.assert_sql(&format!("UPDATE game_creature_template SET trainer_type = 0, trainer_class = {class} WHERE entry = 51001"));
    node.assert_call("playerbots_fixture_provision_reset", &[guid]);
    node.assert_call("playerbots_fixture_provision_steps", &[guid, "32"]);
    let learned = node.query_rows(&format!(
        "SELECT spell_id FROM game_player_spell WHERE character_guid = {guid}"
    ));
    for &(spell, _, _, level) in ranks {
        if level > 20 {
            continue;
        }
        assert!(
            learned
                .iter()
                .any(|row| row["spell_id"] == spell.to_string()),
            "level-20 class {class} did not train {spell}: {learned:?}"
        );
    }
    assert!(
        !learned
            .iter()
            .any(|row| row["spell_id"] == future.to_string()),
        "trained above the Character's level"
    );
    node.assert_sql(&format!(
        "UPDATE game_world_entity SET power = 1000, max_power = 1000 WHERE guid = {guid}"
    ));
    let enemy =
        &node.query_rows("SELECT guid FROM game_creature_spawn WHERE entry = 5098090")[0]["guid"];
    node.assert_call(
        "playerbots_fixture_runner_damage_and_park",
        &[guid, enemy, "1"],
    );
    assert!(
        support::poll_until(Duration::from_secs(10), || {
            node.assert_call("playerbots_fixture_runner_pass_once", &[guid]);
            node.query_rows(&format!(
                "SELECT spell_id FROM pkg_playerbots_action WHERE character_guid = {guid}"
            ))
            .iter()
            .any(|row| row["spell_id"] == expected.to_string())
        }),
        "Runner never selected learned rank {expected} for class {class}: {:?}",
        node.query_rows(&format!(
            "SELECT * FROM pkg_playerbots_runner WHERE character_guid = {guid}"
        ))
    );
    assert!(
        support::poll_until(Duration::from_secs(10), || {
            node.assert_call("playerbots_fixture_runner_pass_once", &[guid]);
            node.query_rows(&format!("SELECT spell_id, is_completion, is_interrupted FROM game_spell_cast_event WHERE caster_guid = {guid}"))
            .iter().any(|row| row["spell_id"] == expected.to_string()
                && row["is_completion"] == "true" && row["is_interrupted"] == "false")
        }),
        "learned rank {expected} was selected but never completed"
    );
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn playerbots_train_eligible_ranks_and_cast_the_highest_known_rank() {
    verify_ranks(
        "8",
        "2",
        &[
            (133, 0, 1, 1),
            (143, 133, 2, 6),
            (145, 143, 3, 12),
            (3140, 145, 4, 18),
            (8400, 3140, 5, 24),
        ],
        3140,
        8400,
    );
    verify_ranks(
        "1",
        "0",
        &[
            (78, 0, 1, 1),
            (284, 78, 2, 8),
            (285, 284, 3, 16),
            (1608, 285, 4, 24),
        ],
        285,
        1608,
    );
    verify_ranks(
        "5",
        "1",
        &[
            (585, 0, 1, 1),
            (591, 585, 2, 6),
            (598, 591, 3, 14),
            (984, 598, 4, 22),
        ],
        598,
        984,
    );
}
