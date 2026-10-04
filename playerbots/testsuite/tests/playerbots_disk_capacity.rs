//! Capacity expiry reaches real spawning and Runner cancellation on a private Standalone.
mod support;

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use support::{poll_until, Standalone};

fn lease(node: &Standalone, until: u128) {
    node.assert_call(
        "set_package_config",
        &[
            "\"playerbots\"",
            "\"capacity_until_micros\"",
            &format!("\"{until}\""),
            "true",
        ],
    );
}

#[test]
#[ignore = "requires the pinned Standalone and Module Wasm"]
fn disk_capacity_expiry_freezes_bots_and_refuses_new_work() {
    let mut node = Standalone::start("playerbots-disk-capacity");
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    let future = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros()
        + 3_600_000_000;
    lease(&node, future);
    node.assert_call(
        "playerbots_spawn_class_role",
        &["1", "1200", "1200", "50", "1", "0"],
    );
    let before = node.query_rows("SELECT character_guid FROM pkg_playerbots_bot");
    let guid = &before[0]["character_guid"];
    lease(&node, 0);
    assert!(
        poll_until(Duration::from_secs(10), || {
            node.query_rows("SELECT controller FROM pkg_playerbots_bot")[0]["controller"]
                .contains("Frozen")
        }),
        "expired lease did not freeze the existing bot"
    );
    for (reducer, args) in [
        (
            "playerbots_spawn_class_role",
            vec!["1", "1200", "1200", "50", "1", "0"],
        ),
        (
            "playerbots_spawn_starting_area",
            vec!["\"northshire\"", "1", "{\"cohort\":[]}"],
        ),
        (
            "playerbots_select_controller",
            vec![guid.as_str(), "{\"cohort\":[]}"],
        ),
    ] {
        let result = node.call(reducer, &args);
        assert!(
            !result.status.success(),
            "{reducer} bypassed the expired lease"
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("capacity lease"));
    }
    assert_eq!(
        node.query_rows("SELECT character_guid FROM pkg_playerbots_bot"),
        before
    );
    lease(&node, future);
    assert!(
        node.query_rows("SELECT controller FROM pkg_playerbots_bot")[0]["controller"]
            .contains("Frozen")
    );
    node.assert_call("playerbots_select_controller", &[guid, "{\"cohort\":[]}"]);
    assert!(
        node.query_rows("SELECT controller FROM pkg_playerbots_bot")[0]["controller"]
            .contains("Cohort")
    );
}
