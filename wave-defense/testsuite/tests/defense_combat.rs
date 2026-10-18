mod support;

use lyracore_shared::spell::taxonomy::{
    A_CONTROL, E_CREATE_ITEM, E_HEAL, E_RECALL_HOME, E_RESURRECT, E_SELF_RESURRECT, E_SUMMON_PET,
    M_ROOT, T_SELF, T_TARGET_ALLY,
};
use support::{number, poll_until, Row, Standalone, POLL_TIMEOUT};

const PACKAGE: &str = "\"wave-defense\"";
const RESURRECT: u32 = 509_9301;
const SELF_RESURRECT: u32 = 509_9302;
const HEAL: u32 = 509_9303;

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn match_admission_blocks_recall_and_partition_exit_until_rules_are_disabled() {
    let (shard, instance) = start("defense-combat-boundary");
    spell(&shard, 5099307, E_RECALL_HOME, T_SELF, 0);
    shard.assert_sql("UPDATE game_character SET home_map = 189, home_x = 90, home_y = 90, home_z = 30 WHERE guid = 1");
    enable(&shard, &instance, &[]);
    let before = entity(&shard, "1");
    refuse(
        &shard,
        "debug_cast_at",
        &["1", "5099307", "1"],
        "Leave the Package match",
    );
    for map in ["189", "0"] {
        shard.assert_call("debug_teleport", &["1", map, "90", "90", "30", "0"]);
        let after = entity(&shard, "1");
        for field in ["map_id", "instance_id", "x", "y", "z", "power"] {
            assert_eq!(before[field], after[field], "blocked exit changed {field}");
        }
    }
    shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "1"]);
    shard.assert_call("debug_cast_at", &["1", "5099307", "1"]);
    assert_eq!(entity(&shard, "1")["instance_id"], "0");
    assert_eq!(number::<f32>(&entity(&shard, "1"), "x"), 90.0);
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn item_creation_preserves_match_reagents_and_normal_casts_still_require_them() {
    let (shard, instance) = start("defense-combat-reagents");
    let spell_id = 5099306;
    spell(&shard, spell_id, E_CREATE_ITEM, T_SELF, 0);
    shard.assert_sql("UPDATE game_spell_effect SET p0 = 5090052, base_points = 1, die_sides = 0 WHERE spell_id = 5099306");
    shard.assert_sql(&format!("INSERT INTO game_spell_reagent (id,spell_id,item_entry,count) VALUES ({},5099306,5090054,1)",u64::from(spell_id)<<3));
    refuse(
        &shard,
        "debug_cast_at",
        &["1", "5099306", "1"],
        "missing 1 of item 5090054",
    );
    enable(&shard, &instance, &[]);
    shard.assert_call("debug_cast_at", &["1", "5099306", "1"]);
    assert_eq!(
        shard.query_rows(
            "SELECT stack_count FROM game_item_instance WHERE owner_guid = 1 AND entry = 5090052"
        )[0]["stack_count"],
        "1"
    );
    shard.assert_call("debug_grant_item", &["1", "5090054", "1"]);
    shard.assert_call("debug_cast_at", &["1", "5099306", "1"]);
    assert_eq!(
        shard.query_rows(
            "SELECT stack_count FROM game_item_instance WHERE owner_guid = 1 AND entry = 5090054"
        )[0]["stack_count"],
        "1"
    );
    shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "1"]);
    shard.assert_call("debug_cast_at", &["1", "5099306", "1"]);
    assert!(shard
        .query_rows("SELECT * FROM game_item_instance WHERE owner_guid = 1 AND entry = 5090054")
        .is_empty());
    refuse(
        &shard,
        "debug_cast_at",
        &["1", "5099306", "1"],
        "missing 1 of item 5090054",
    );
}

fn start(name: &str) -> (Standalone, String) {
    let mut shard = Standalone::start(name);
    shard.publish_module();
    shard.assert_call("claim_operator", &[]);
    shard.assert_call("install_guid_range", &["0"]);
    shard.assert_call("debug_seed_scenario_fixtures", &[]);
    shard.assert_call(
        "debug_stage_meeting_stone_character",
        &["2", "5099302", "1", "1", "1", "189", "10", "20", "30"],
    );
    shard.assert_call("debug_stage_meeting_stone_group", &["1", "[2]", "false"]);
    let group = shard.query_rows("SELECT group_id FROM game_group WHERE leader_guid = 1")[0]
        ["group_id"]
        .clone();
    shard.assert_call("debug_defense_create_instance", &[PACKAGE, "189", &group]);
    let instance = shard.query_rows(&format!(
        "SELECT instance_id FROM game_instance WHERE party_id = {group}"
    ))[0]["instance_id"]
        .clone();
    shard.assert_sql(
        "UPDATE game_character SET online = true, map_id = 189, level = 1 WHERE guid = 1",
    );
    shard.assert_call("debug_spawn_player_entity", &["1"]);
    for guid in ["1", "2"] {
        shard.assert_call("debug_defense_enter_instance", &[PACKAGE, guid, &instance]);
        shard.assert_sql(&format!("UPDATE game_world_entity SET simulation_indexed = false, max_health = 10000, health = 10000, level = 1 WHERE guid = {guid}"));
    }
    (shard, instance)
}

fn enable(shard: &Standalone, instance: &str, spells: &[u32]) {
    shard.assert_call(
        "debug_defense_enable_combat_rule",
        &[
            PACKAGE,
            "1",
            instance,
            &serde_json::to_string(spells).unwrap(),
        ],
    );
}

fn entity(shard: &Standalone, guid: &str) -> Row {
    shard.query_rows(&format!(
        "SELECT * FROM game_world_entity WHERE guid = {guid}"
    ))[0]
        .clone()
}

fn refuse(shard: &Standalone, reducer: &str, args: &[&str], reason: &str) {
    let output = shard.call(reducer, args);
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains(reason), "{text}");
}

fn spell(shard: &Standalone, id: u32, kind: u8, target: u8, level: u8) {
    shard.assert_sql(&format!("INSERT INTO game_spell (spell_id,name,power_type,cost,cast_time_ms,gcd_ms,cooldown_ms,range_yd,duration_ms,school_mask,dispel_type,mechanic,max_stacks,aura_interrupt,attributes,spell_level,max_level,is_negative,cast_flags,stances,family_name,family_flags,proc_flags,proc_chance,proc_charges) VALUES ({id},'Fixture spell',0,0,0,0,0,40,0,2,0,0,0,0,0,{level},0,false,0,0,0,0,0,0,0)"));
    shard.assert_sql(&format!("INSERT INTO game_spell_effect (id,spell_id,effect_index,kind,base_points,die_sides,per_level,period_ms,target,radius_yd,chain_targets,trigger_spell,effect_mechanic,p0,p0_kind,p1,script_id,enters_combat) VALUES ({},{id},0,{kind},20,1,0.0,0,{target},0.0,0,0,0,0,0,0,0,false)",u64::from(id)<<2));
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn mixed_faction_teammates_can_heal_and_leaving_restores_the_original_faction() {
    let (shard, instance) = start("defense-combat-faction");
    for (id, faction, group, friends, enemies) in [(1, 1, 3, 2, 12), (2, 2, 5, 4, 10)] {
        shard.assert_sql(&format!("INSERT INTO game_faction_template (id,faction,faction_group,friend_group,enemy_group,enemy_0,enemy_1,enemy_2,enemy_3,friend_0,friend_1,friend_2,friend_3) VALUES ({id},{faction},{group},{friends},{enemies},0,0,0,0,0,0,0,0)"));
    }
    shard.assert_sql("UPDATE game_world_entity SET faction_template = 1 WHERE guid = 1");
    shard.assert_sql(
        "UPDATE game_world_entity SET faction_template = 2, health = 1000 WHERE guid = 2",
    );
    spell(&shard, HEAL, E_HEAL, T_TARGET_ALLY, 0);
    refuse(
        &shard,
        "debug_cast_at",
        &["1", &HEAL.to_string(), "2"],
        "targets allies",
    );
    enable(&shard, &instance, &[]);
    for _ in 0..2 {
        shard.assert_call(
            "debug_defense_enable_combat_rule",
            &[PACKAGE, "2", &instance, "[]"],
        );
    }
    assert_eq!(entity(&shard, "2")["faction_template"], "1");
    assert_eq!(shard.query_rows("SELECT original_faction_template FROM game_package_combat_rule WHERE character_guid = 2")[0]["original_faction_template"], "2");
    shard.assert_call("debug_cast_at", &["1", &HEAL.to_string(), "2"]);
    assert!(number::<u32>(&entity(&shard, "2"), "health") > 1000);
    shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "2"]);
    assert_eq!(entity(&shard, "2")["faction_template"], "2");
    refuse(
        &shard,
        "debug_cast_at",
        &["1", &HEAL.to_string(), "2"],
        "targets allies",
    );
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn match_pets_inherit_the_team_faction_and_package_teardown_restores_live_bodies() {
    let (shard, instance) = start("defense-combat-pet-faction");
    shard.assert_sql("UPDATE game_world_entity SET faction_template = 2 WHERE guid = 1");
    enable(&shard, &instance, &[]);
    spell(&shard, 5099305, E_SUMMON_PET, T_SELF, 0);
    shard.assert_sql("UPDATE game_spell_effect SET p0 = 51000 WHERE spell_id = 5099305");
    shard.assert_call("debug_cast_at", &["1", "5099305", "1"]);
    let pets = shard.query_rows("SELECT * FROM game_world_entity WHERE owner_guid = 1");
    assert_eq!(pets.len(), 1);
    assert_eq!(pets[0]["faction_template"], "1");
    shard.assert_call("teardown_package", &[PACKAGE]);
    assert_eq!(entity(&shard, "1")["faction_template"], "2");
    assert_eq!(entity(&shard, &pets[0]["guid"])["faction_template"], "2");
    assert!(shard
        .query_rows("SELECT * FROM game_package_combat_rule")
        .is_empty());
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn match_deaths_preserve_durability_and_disabling_rules_restores_normal_loss() {
    let (shard, instance) = start("defense-combat-durability");
    shard.assert_call("debug_grant_item", &["1", "5090050", "1"]);
    shard.assert_sql("UPDATE game_item_template SET max_durability = 100 WHERE entry = 5090050");
    shard.assert_sql("UPDATE game_item_instance SET slot = 15, durability = 100 WHERE owner_guid = 1 AND entry = 5090050");
    enable(&shard, &instance, &[]);
    shard.assert_call("debug_set_health", &["1", "0"]);
    assert_eq!(
        shard.query_rows(
            "SELECT durability FROM game_item_instance WHERE owner_guid = 1 AND entry = 5090050"
        )[0]["durability"],
        "100"
    );
    shard.assert_sql("UPDATE game_world_entity SET dead = false, health = 10000 WHERE guid = 1");
    shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "1"]);
    shard.assert_call("debug_set_health", &["1", "0"]);
    assert_eq!(
        shard.query_rows(
            "SELECT durability FROM game_item_instance WHERE owner_guid = 1 AND entry = 5090050"
        )[0]["durability"],
        "90"
    );
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn resurrection_shares_two_minutes_with_self_resurrection_and_only_entitled_spells_ignore_level() {
    let (shard, instance) = start("defense-combat-resurrection");
    spell(&shard, RESURRECT, E_RESURRECT, T_TARGET_ALLY, 20);
    spell(&shard, SELF_RESURRECT, E_SELF_RESURRECT, T_SELF, 0);
    spell(&shard, HEAL, E_HEAL, T_SELF, 20);
    enable(&shard, &instance, &[RESURRECT]);
    refuse(
        &shard,
        "debug_cast_at",
        &["1", &HEAL.to_string(), "1"],
        "requires level 20",
    );
    shard.assert_call("debug_set_health", &["2", "0"]);
    shard.assert_call("debug_cast_at", &["1", &RESURRECT.to_string(), "2"]);
    assert_eq!(
        shard
            .query_rows("SELECT target_guid FROM game_resurrect_request WHERE target_guid = 2")
            .len(),
        1
    );
    shard.assert_call("debug_set_health", &["1", "0"]);
    shard.assert_sql(&format!("INSERT INTO game_self_resurrect_option (character_guid,spell_id) VALUES (1,{SELF_RESURRECT})"));
    refuse(&shard, "debug_self_resurrect", &["1"], "Package cooldown");
    shard.assert_sql("UPDATE game_package_combat_rule SET resurrect_ready_at_micros = 0 WHERE character_guid = 1");
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    shard.assert_call("debug_self_resurrect", &["1"]);
    assert_eq!(entity(&shard, "1")["dead"], "false");
    refuse(
        &shard,
        "debug_cast_at",
        &["1", &RESURRECT.to_string(), "2"],
        "Package cooldown",
    );
    let ready: i64 = number(&shard.query_rows("SELECT resurrect_ready_at_micros FROM game_package_combat_rule WHERE character_guid = 1")[0], "resurrect_ready_at_micros");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    assert!(ready >= started + 120_000_000 && ready <= now + 120_000_000);
    shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "1"]);
    refuse(
        &shard,
        "debug_cast_at",
        &["1", &RESURRECT.to_string(), "2"],
        "requires level 20",
    );
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn match_ranged_attacks_work_without_ammo_and_leave_loaded_ammo_unchanged() {
    let (shard, instance) = start("defense-combat-ammo");
    shard.assert_call("debug_grant_item", &["1", "5090050", "1"]);
    shard.assert_sql("UPDATE game_item_template SET class = 2, subclass = 2, inventory_type = 15, damage_min = 1.0, damage_max = 1.0, delay_ms = 1000 WHERE entry = 5090050");
    shard.assert_sql(
        "UPDATE game_item_instance SET slot = 17 WHERE owner_guid = 1 AND entry = 5090050",
    );
    shard.assert_call("debug_spawn_at_feet", &["1", "51000", "15"]);
    let target = shard.query_rows(&format!(
        "SELECT guid FROM game_world_entity WHERE entry = 51000 AND instance_id = {instance}"
    ))[0]["guid"]
        .clone();
    shard.assert_sql(&format!("UPDATE game_world_entity SET simulation_indexed = false, health = 1000000, max_health = 1000000 WHERE guid = {target}"));
    spell(&shard, 5099304, A_CONTROL, T_SELF, 0);
    shard.assert_sql("UPDATE game_spell SET duration_ms = 600000 WHERE spell_id = 5099304");
    shard.assert_sql(&format!(
        "UPDATE game_spell_effect SET p0 = {M_ROOT} WHERE spell_id = 5099304"
    ));
    shard.assert_call("debug_cast_at", &[&target, "5099304", &target]);
    refuse(
        &shard,
        "debug_ranged_attack_nearest",
        &["1", "75"],
        "no ammo",
    );
    enable(&shard, &instance, &[]);
    shard.assert_call("debug_ranged_attack_nearest", &["1", "75"]);
    assert!(poll_until(POLL_TIMEOUT, || number::<u32>(
        &entity(&shard, &target),
        "health"
    ) < 1000000));
    shard.assert_sql(
        "UPDATE game_item_template SET class = 6, max_stack = 200 WHERE entry = 5090052",
    );
    shard.assert_call("debug_grant_item", &["1", "5090052", "20"]);
    let before: u32 = number(&entity(&shard, &target), "health");
    assert!(poll_until(POLL_TIMEOUT, || number::<u32>(
        &entity(&shard, &target),
        "health"
    ) < before));
    assert_eq!(
        shard.query_rows(
            "SELECT stack_count FROM game_item_instance WHERE owner_guid = 1 AND entry = 5090052"
        )[0]["stack_count"],
        "20"
    );
    shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "1"]);
    assert!(poll_until(POLL_TIMEOUT, || number::<u32>(
        &shard.query_rows(
            "SELECT stack_count FROM game_item_instance WHERE owner_guid = 1 AND entry = 5090052"
        )[0],
        "stack_count"
    ) < 20));
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn match_kills_leave_shared_xp_to_the_package_and_normal_kills_still_award_xp() {
    let (shard, instance) = start("defense-combat-xp");
    enable(&shard, &instance, &[]);
    for protected in [true, false] {
        if !protected {
            shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "1"]);
        }
        shard.assert_call("debug_spawn_at_feet", &["1", "51000", "0"]);
        let target = shard.query_rows(&format!("SELECT guid FROM game_world_entity WHERE entry = 51000 AND instance_id = {instance} AND dead = false"))[0]["guid"].clone();
        shard.assert_call(
            "debug_apply_lethal_damage_floor_fixture",
            &["1", &target, "1"],
        );
        shard.assert_call("debug_kill_creature", &["1", &target]);
        let xp: u32 = number(&entity(&shard, "1"), "xp");
        if protected {
            assert_eq!(xp, 0);
        } else {
            assert!(xp > 0);
        }
    }
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn manual_talent_spending_is_refused_only_while_package_combat_rules_apply() {
    let (shard, instance) = start("defense-combat-talents");
    spell(&shard, HEAL, E_HEAL, T_SELF, 0);
    shard.assert_sql(&format!("INSERT INTO game_talent (talent_id,name,tree_id,tier,column,max_rank,spell_id,required_talent_id,required_points_in_tree,grant_spell_id,tab_id,rank_spell_2,rank_spell_3,rank_spell_4,rank_spell_5,required_talent_rank,required_spell_id) VALUES (5099310,'Fixture talent',0,0,0,1,{HEAL},0,0,{HEAL},0,0,0,0,0,0,0)"));
    shard.assert_sql("UPDATE game_world_entity SET level = 10 WHERE guid = 1");
    enable(&shard, &instance, &[]);
    refuse(
        &shard,
        "debug_learn_talent",
        &["1", "5099310"],
        "through the Package",
    );
    assert!(shard
        .query_rows("SELECT rank FROM game_character_talent WHERE character_guid = 1")
        .is_empty());
    shard.assert_call("debug_defense_disable_combat_rule", &[PACKAGE, "1"]);
    shard.assert_call("debug_learn_talent", &["1", "5099310"]);
    assert_eq!(
        shard.query_rows("SELECT rank FROM game_character_talent WHERE character_guid = 1")[0]
            ["rank"],
        "1"
    );
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn combat_rules_stop_outside_the_instance_and_after_package_teardown() {
    let (shard, instance) = start("defense-combat-scope");
    spell(&shard, HEAL, E_HEAL, T_SELF, 20);
    enable(&shard, &instance, &[HEAL]);
    shard.assert_call("debug_cast_at", &["1", &HEAL.to_string(), "1"]);
    shard.assert_sql("UPDATE game_world_entity SET instance_id = 0 WHERE guid = 1");
    refuse(
        &shard,
        "debug_cast_at",
        &["1", &HEAL.to_string(), "1"],
        "requires level 20",
    );
    shard.assert_call("debug_defense_enter_instance", &[PACKAGE, "1", &instance]);
    shard.assert_call("debug_cast_at", &["1", &HEAL.to_string(), "1"]);
    shard.assert_call("teardown_package", &[PACKAGE]);
    refuse(
        &shard,
        "debug_cast_at",
        &["1", &HEAL.to_string(), "1"],
        "requires level 20",
    );
    refuse(
        &shard,
        "debug_defense_enable_combat_rule",
        &[PACKAGE, "1", &instance, "[]"],
        "enabled Package",
    );
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI and Wasm toolchain"]
fn a_learned_essential_reincarnation_provides_a_self_resurrection_option() {
    let (shard, instance) = start("defense-combat-reincarnation");
    spell(&shard, 20608, E_HEAL, T_SELF, 0);
    spell(&shard, 21169, E_SELF_RESURRECT, T_SELF, 0);
    shard.assert_call("debug_learn_spell", &["1", "20608"]);
    enable(&shard, &instance, &[20608]);
    shard.assert_call("debug_set_health", &["1", "0"]);
    assert_eq!(
        shard
            .query_rows("SELECT spell_id FROM game_self_resurrect_option WHERE character_guid = 1")
            [0]["spell_id"],
        "21169"
    );
    shard.assert_call("debug_self_resurrect", &["1"]);
    assert_eq!(entity(&shard, "1")["dead"], "false");
    shard.assert_call("debug_set_health", &["1", "0"]);
    refuse(&shard, "debug_self_resurrect", &["1"], "Package cooldown");
}
