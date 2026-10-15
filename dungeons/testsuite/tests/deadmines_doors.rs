use std::time::Duration;

use lyracore_test_support::{poll_until, Standalone};

fn boss_doors(node: &Standalone, map_id: u32, instance_id: u64) -> Vec<(u32, u8)> {
    let mut doors: Vec<_> = node
        .query_rows(&format!(
            "SELECT template_entry, state FROM game_gameobject WHERE instance_id = {instance_id} AND map_id = {map_id}"
        ))
        .into_iter()
        .filter_map(|row| {
            let entry = row["template_entry"].parse::<u32>().unwrap();
            [13965, 16400, 16399]
                .contains(&entry)
                .then(|| (entry, row["state"].parse::<u8>().unwrap()))
        })
        .collect();
    doors.sort_unstable();
    doors
}

fn completed_bosses(node: &Standalone) -> Vec<(u32, u8)> {
    let mut states: Vec<_> = node
        .query_rows("SELECT encounter_id, state FROM game_encounter_state WHERE instance_id = 936")
        .into_iter()
        .filter_map(|row| {
            let encounter_id = row["encounter_id"].parse::<u32>().unwrap();
            (encounter_id < 3).then(|| (encounter_id, row["state"].parse::<u8>().unwrap()))
        })
        .collect();
    states.sort_unstable();
    states
}

#[test]
#[ignore = "requires the pinned Standalone, Module Wasm, and this dungeons Package installed"]
fn deadmines_completed_boss_doors_reopen_after_restart_and_recreation() {
    let mut node = Standalone::start_persistent("deadmines-door-reload");
    node.publish_module();
    for stage in [
        "debug_deadmines_begin",
        "debug_deadmines_rhahkzor_falls",
        "debug_deadmines_shredder_ejects_sneed",
        "debug_deadmines_sneed_falls",
        "debug_deadmines_gilnid_falls",
        "debug_deadmines_prepare_door_reload",
    ] {
        node.assert_call(stage, &[]);
    }
    assert_eq!(completed_bosses(&node), [(0, 2), (1, 2), (2, 2)]);
    assert!(boss_doors(&node, 36, 936).is_empty());

    node.restart_persistent();
    assert_eq!(completed_bosses(&node), [(0, 2), (1, 2), (2, 2)]);
    assert!(boss_doors(&node, 36, 936).is_empty());

    for _ in 0..2 {
        node.assert_call("debug_deadmines_recreate_boss_doors", &[]);
        assert!(
            poll_until(Duration::from_secs(10), || {
                boss_doors(&node, 36, 936) == [(13965, 1), (16399, 1), (16400, 1)]
            }),
            "recreated boss doors did not reopen from saved completion"
        );
        for (map_id, instance_id) in [(36, 937), (1, 938), (36, 0)] {
            assert_eq!(
                boss_doors(&node, map_id, instance_id),
                [(13965, 0), (16399, 0), (16400, 0)],
                "door restoration escaped the completed encounter"
            );
        }
        assert_eq!(completed_bosses(&node), [(0, 2), (1, 2), (2, 2)]);
    }
}
