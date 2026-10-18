mod support;

use std::path::PathBuf;
use std::process::Command;

use lyracore_test_support::{core_root, Standalone};
use serde_json::Value;

fn import(node: &Standalone, arguments: &[&str]) {
    let output = support::private_importer(node, arguments);
    assert!(
        output.status.success(),
        "import {arguments:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sql_value(value: &Value) -> String {
    match value {
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''")),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => panic!("Module row contains a non-scalar value"),
    }
}

#[test]
#[ignore = "requires private SpacetimeDB, actual client MPQs, a ClassicDB dump, and mapped Module content JSON"]
fn imported_courtyard_passes_all_mode_routes_and_configures_once() {
    let client = std::env::var("LYRACORE_TEST_CLIENT_DATA")
        .expect("actual client Data directory is required");
    let dump = std::env::var("LYRACORE_TEST_WORLD_DUMP").expect("ClassicDB SQL dump is required");
    let input = std::env::var("WAVE_DEFENSE_BASE_CONTENT")
        .expect("actual mapped Module content JSON is required");
    let base: Value = serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    let directory =
        std::env::temp_dir().join(format!("wave-defense-courtyard-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let output = Command::new("bun")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../datascripts/defense.ts"))
        .env("WAVE_DEFENSE_BASE_CONTENT", &input)
        .env("LYRACORE_CORE_ROOT", core_root())
        .env("LYRACORE_PACKAGES_ROOT", &directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut node = Standalone::start("wave-defense-real-courtyard");
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    import(&node, &["--dbc", &client, "--taxi-mounts", &dump]);
    import(&node, &["--dbc", &client, "--spells"]);
    import(&node, &["--dbc", &client, "--talents"]);
    let packages = directory.to_str().unwrap();
    import(
        &node,
        &[
            "--dump",
            &dump,
            "--dbc",
            &client,
            "--world-profile",
            "scarlet-courtyard",
            "--family",
            "creatures",
            "--packages",
            packages,
        ],
    );
    import(
        &node,
        &[
            "--dump",
            &dump,
            "--dbc",
            &client,
            "--world-profile",
            "scarlet-courtyard",
            "--family",
            "globals",
            "--packages",
            packages,
        ],
    );
    for source in [5175, 5176, 5177, 5178] {
        let row = base["tables"]["game_item_template"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["entry"] == source)
            .expect("mapped source JSON must include all four Shaman totem items")
            .as_object()
            .unwrap();
        let columns = std::iter::once("entry")
            .chain(
                lyracore_package_delta::Table::Item
                    .columns()
                    .iter()
                    .map(|column| column.name),
            )
            .chain(std::iter::once("random_property"))
            .collect::<Vec<_>>();
        node.assert_sql(&format!(
            "INSERT INTO game_item_template ({}) VALUES ({})",
            columns.join(","),
            columns
                .iter()
                .map(|column| sql_value(&row[*column]))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    let items: Value = serde_json::from_slice(
        &std::fs::read(directory.join("wave-defense/data/.generated/items.json")).unwrap(),
    )
    .unwrap();
    let items = serde_json::to_string(&items).unwrap();
    node.assert_call(
        "apply_package_deltas",
        &["\"items\"", &serde_json::to_string(&items).unwrap()],
    );
    import(
        &node,
        &["--vmap", &client, "--world-profile", "scarlet-courtyard"],
    );
    let geometry =
        node.query_rows("SELECT * FROM game_vmap_generation WHERE map_id = 189 AND state = 2");
    assert_eq!(geometry.len(), 1);
    let generation = &geometry[0]["id"];
    import(&node, &["--vmap-prepare-coverage", generation]);
    node.assert_call("debug_set_nav_enabled", &["true"]);
    node.assert_call("debug_set_vmap_enabled", &["true"]);
    node.assert_call("debug_set_nav_coverage_enabled", &["true"]);
    for offset in [-12.0, -6.0, -3.0, 0.0, 3.0, 6.0, 12.0] {
        node.assert_call(
            "debug_floor_probe",
            &["189", "909.251", &(1378.57 + offset).to_string(), "18.0794"],
        );
    }
    let logs = node.module_logs(50);
    let floors = String::from_utf8_lossy(&logs.stdout)
        .lines()
        .filter(|line| line.contains("floor probe"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    println!("Courtyard floors: {}", floors.join("\n"));
    node.assert_call("complete_world_import", &["[189]"]);
    node.assert_call(
        "gw_certify_package_party_authority",
        &["\"courtyard-test\"", "true"],
    );
    node.assert_call("wave_defense_configure", &["true"]);
    let config = node.query_rows("SELECT * FROM pkg_wave_defense_config");
    assert_eq!(config.len(), 1);
    assert_eq!(config[0]["enabled"], "true");
    assert_ne!(config[0]["lobby_instance_id"], "0");
    let stock = node.query_rows("SELECT * FROM game_npc_vendor WHERE creature_entry = 15189003");
    assert_eq!(stock.len(), 51);
    node.assert_call("wave_defense_configure", &["true"]);
    assert_eq!(
        node.query_rows("SELECT * FROM pkg_wave_defense_config"),
        config
    );
    assert_eq!(
        node.query_rows("SELECT * FROM game_npc_vendor WHERE creature_entry = 15189003"),
        stock
    );
    assert_eq!(
        node.query_rows("SELECT * FROM pkg_wave_defense_schedule")
            .len(),
        1
    );
    let evidence = serde_json::json!({
        "source_sha": base["source_sha"],
        "geometry": geometry,
        "coverage": node.query_rows("SELECT * FROM game_vmap_nav_coverage_manifest"),
        "config": config,
        "shop_items": stock.len(),
        "floor_probes": floors,
        "accepted_routes": ["center front", "south front", "north front", "quartermaster"],
    });
    std::fs::write(
        directory.join("evidence.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    node.assert_call("wave_defense_configure", &["false"]);
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_schedule")
        .is_empty());
    println!(
        "Courtyard evidence: {}",
        directory.join("evidence.json").display()
    );
}
