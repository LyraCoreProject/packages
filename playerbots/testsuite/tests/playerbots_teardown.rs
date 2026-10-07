//! Core's Package Teardown run on a populated private Shard: the bots stay as Dormant Characters,
//! every Package table empties, and the brain pass does not seed them again.

mod support;

use std::time::Duration;

use support::{poll_until, Standalone, POLL_TIMEOUT};

/// Empty ground on the open-world map. Positive, because `spacetime call` reads a leading `-` as a
/// flag.
const SPAWN_AT: (&str, &str, &str) = ("1200.0", "1200.0", "50.0");

/// Every production table. The brain pass writes most of them on each tick, and seeds the kit,
/// rotation and catalog tables on its first.
const TABLES: &[&str] = &[
    "pkg_playerbots_action",
    "pkg_playerbots_bot",
    "pkg_playerbots_catalog_objective",
    "pkg_playerbots_catalog_quest",
    "pkg_playerbots_catalog_seed",
    "pkg_playerbots_companion_order",
    "pkg_playerbots_goal",
    "pkg_playerbots_kit",
    "pkg_playerbots_movement",
    "pkg_playerbots_personality",
    "pkg_playerbots_provisioning",
    "pkg_playerbots_quest_admission",
    "pkg_playerbots_quest_catalog",
    "pkg_playerbots_quest_objective",
    "pkg_playerbots_recovery_scan",
    "pkg_playerbots_rotation",
    "pkg_playerbots_runner",
    "pkg_playerbots_scheduler",
];

fn arg(value: &str) -> String {
    serde_json::to_string(value).expect("a string encodes as JSON")
}

#[test]
#[ignore = "requires the SpacetimeDB 2.7.1 CLI, the Wasm toolchain, and the playerbots Package installed (lyracore packages add playerbots)"]
fn teardown_leaves_every_bot_dormant_and_every_package_table_empty() {
    let mut node = Standalone::start("playerbots-teardown");
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    node.assert_call(
        "playerbots_spawn",
        &["3", SPAWN_AT.0, SPAWN_AT.1, SPAWN_AT.2],
    );
    assert!(
        poll_until(POLL_TIMEOUT, || !node
            .query_rows("SELECT * FROM pkg_playerbots_scheduler")
            .is_empty()),
        "the brain pass never ran"
    );
    let bots: Vec<String> = node
        .query_rows("SELECT character_guid FROM pkg_playerbots_bot")
        .into_iter()
        .map(|row| row["character_guid"].clone())
        .collect();
    assert_eq!(bots.len(), 3);

    node.assert_call("teardown_package", &[&arg("playerbots")]);
    // Several brain-pass ticks. A pass that still ran would seed the kit and scheduler again.
    std::thread::sleep(Duration::from_secs(3));

    for table in TABLES {
        let rows = node.query_rows(&format!("SELECT * FROM {table}"));
        assert!(
            rows.is_empty(),
            "{table} holds rows after teardown: {rows:?}"
        );
    }
    assert!(
        node.query_rows("SELECT * FROM game_package_config WHERE package_name = 'playerbots'")
            .is_empty(),
        "teardown kept the Package Config"
    );
    for bot in &bots {
        let character = node.query_rows(&format!(
            "SELECT online FROM game_character WHERE guid = {bot}"
        ));
        assert_eq!(character.len(), 1, "bot {bot} lost its Character");
        assert_eq!(character[0]["online"], "false", "bot {bot} is online");
        for (what, query) in [
            ("live entity", format!("SELECT guid FROM game_world_entity WHERE guid = {bot}")),
            (
                "Sessionless Action Consent",
                format!(
                    "SELECT character_guid FROM game_sessionless_action_consent WHERE character_guid = {bot}"
                ),
            ),
        ] {
            assert!(
                node.query_rows(&query).is_empty(),
                "bot {bot} kept its {what}"
            );
        }
    }
}
