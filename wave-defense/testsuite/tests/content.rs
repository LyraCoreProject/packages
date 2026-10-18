use std::path::PathBuf;
use std::process::Command;

use lyracore_package_delta::PackageDelta;
use lyracore_test_support::{core_root, Standalone};
use serde_json::Value;

fn private_shard(name: &str) -> Standalone {
    let mut node = Standalone::start(name);
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node
}

#[test]
#[ignore = "requires bun and WAVE_DEFENSE_BASE_CONTENT with offline imported Module rows"]
fn authored_content_uses_owned_identifiers_and_affordable_class_gear() {
    let input = std::env::var_os("WAVE_DEFENSE_BASE_CONTENT")
        .expect("offline Module content JSON input is required");
    let directory =
        std::env::temp_dir().join(format!("wave-defense-content-{}", std::process::id()));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../datascripts/defense.ts");
    let output = Command::new("bun")
        .arg(script)
        .env("WAVE_DEFENSE_BASE_CONTENT", input)
        .env("LYRACORE_CORE_ROOT", core_root())
        .env("LYRACORE_PACKAGES_ROOT", &directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let generated = directory.join("wave-defense/data/.generated");
    let creatures = std::fs::read_to_string(generated.join("creatures.json")).unwrap();
    let items = std::fs::read_to_string(generated.join("items.json")).unwrap();
    PackageDelta::parse(&creatures).unwrap();
    PackageDelta::parse(&items).unwrap();
    let creatures: Value = serde_json::from_str(&creatures).unwrap();
    let items: Value = serde_json::from_str(&items).unwrap();
    let creatures = creatures["claims"].as_array().unwrap();
    assert_eq!(creatures.len(), 4);
    for creature in creatures {
        assert!((15_000_000..16_000_000).contains(&creature["key"]["entry"].as_u64().unwrap()));
        assert_eq!(creature["operation"], "insert");
    }
    assert_eq!(creatures[0]["fields"]["creature_type"]["value"], 6);
    assert_eq!(creatures[1]["fields"]["creature_type"]["value"], 6);
    assert_eq!(creatures[2]["fields"]["npc_flags"]["value"], 128);
    assert_eq!(creatures[3]["fields"]["creature_type"]["value"], 1);
    assert_eq!(creatures[3]["fields"]["creature_family"]["value"], 1);
    let items = items["claims"].as_array().unwrap();
    assert_eq!(items.len(), 51);
    for item in items {
        assert!((7_000_000..8_000_000).contains(&item["key"]["entry"].as_u64().unwrap()));
        assert_eq!(item["operation"], "insert");
        assert_eq!(item["fields"]["sell_price"]["value"], 0);
        assert!(item["fields"]["buy_price"]["value"].as_u64().unwrap() <= 80_000);
    }
    for (level, price, count) in [
        (10, 5_000, 10),
        (25, 20_000, 10),
        (40, 50_000, 11),
        (55, 80_000, 11),
    ] {
        let tier = items
            .iter()
            .filter(|item| {
                item["fields"]["required_level"]["value"] == level
                    && item["fields"]["buy_price"]["value"] == price
            })
            .collect::<Vec<_>>();
        assert_eq!(tier.len(), count);
        for (class, subclass) in [
            (2, 7),
            (2, 10),
            (2, 15),
            (2, 2),
            (2, 19),
            (2, 5),
            (4, 6),
            (4, 1),
            (4, 2),
            (4, 3),
        ] {
            assert!(
                tier.iter()
                    .any(|item| item["fields"]["class"]["value"] == class
                        && item["fields"]["subclass"]["value"] == subclass),
                "level {level} misses class {class} subclass {subclass}"
            );
        }
    }
    assert_eq!(items[50]["fields"]["buy_price"]["value"], 0);
    assert_eq!(items[50]["fields"]["buy_count"]["value"], 200);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires private SpacetimeDB"]
fn configure_refuses_missing_map_before_changing_mode_state() {
    let node = private_shard("defense-config-preflight");
    node.assert_call(
        "gw_certify_package_party_authority",
        &["\"defense-content-test\"", "true"],
    );
    let stock = node.query_rows("SELECT * FROM game_npc_vendor");
    let output = node.call("wave_defense_configure", &["true"]);
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success() && message.contains("Import map 189"),
        "{message}"
    );
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_config")
        .is_empty());
    assert!(node
        .query_rows("SELECT * FROM pkg_wave_defense_schedule")
        .is_empty());
    assert_eq!(node.query_rows("SELECT * FROM game_npc_vendor"), stock);
}

#[test]
#[ignore = "requires private SpacetimeDB"]
fn quartermaster_stock_is_idempotent_and_preserves_other_vendors() {
    let node = private_shard("defense-quartermaster-stock");
    node.assert_sql("INSERT INTO game_npc_vendor (id,creature_entry,item_entry,slot,max_count) VALUES (918900000,51000,25,1,0)");
    let other = node.query_rows("SELECT * FROM game_npc_vendor WHERE creature_entry = 51000");
    node.assert_call("wave_defense_content_fixture_stock", &[]);
    let stock = node.query_rows("SELECT * FROM game_npc_vendor WHERE creature_entry = 15189003");
    assert_eq!(stock.len(), 51);
    node.assert_call("wave_defense_content_fixture_stock", &[]);
    assert_eq!(
        node.query_rows("SELECT * FROM game_npc_vendor WHERE creature_entry = 15189003"),
        stock
    );
    assert_eq!(
        node.query_rows("SELECT * FROM game_npc_vendor WHERE creature_entry = 51000"),
        other
    );
}

#[test]
#[ignore = "requires private SpacetimeDB"]
fn quartermaster_refuses_foreign_stock_identifiers_without_changes() {
    let node = private_shard("defense-quartermaster-collision");
    node.assert_sql("INSERT INTO game_npc_vendor (id,creature_entry,item_entry,slot,max_count) VALUES (15189003000,51000,25,1,0)");
    let before = node.query_rows("SELECT * FROM game_npc_vendor");
    let output = node.call("wave_defense_content_fixture_stock", &[]);
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success() && message.contains("belong to another vendor"),
        "{message}"
    );
    assert_eq!(node.query_rows("SELECT * FROM game_npc_vendor"), before);
}
