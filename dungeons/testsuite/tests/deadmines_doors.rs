use lyracore_test_support::Standalone;

fn doors(node: &Standalone) -> Vec<(u32, u8)> {
    let mut doors: Vec<_> = node
        .query_rows(
            "SELECT template_entry, state FROM game_gameobject WHERE instance_id = 936 AND map_id = 36",
        )
        .into_iter()
        .filter_map(|row| {
            let entry = row["template_entry"].parse::<u32>().unwrap();
            [13965, 16397, 16399, 16400]
                .contains(&entry)
                .then(|| (entry, row["state"].parse::<u8>().unwrap()))
        })
        .collect();
    doors.sort_unstable();
    doors
}

fn door_encounters(node: &Standalone) -> Vec<(u32, u8)> {
    let mut states: Vec<_> = node
        .query_rows("SELECT encounter_id, state FROM game_encounter_state WHERE instance_id = 936")
        .into_iter()
        .filter_map(|row| {
            let encounter_id = row["encounter_id"].parse::<u32>().unwrap();
            [0, 1, 2, 4]
                .contains(&encounter_id)
                .then(|| (encounter_id, row["state"].parse::<u8>().unwrap()))
        })
        .collect();
    states.sort_unstable();
    states
}

#[test]
#[ignore = "requires the pinned Standalone, Module Wasm, and this dungeons Package installed"]
fn deadmines_doors_and_encounter_completion_survive_restart() {
    let mut node = Standalone::start_persistent("deadmines-door-persistence");
    node.publish_module();
    node.assert_call("debug_deadmines_begin", &[]);
    assert_eq!(
        doors(&node),
        [(13965, 0), (16397, 0), (16399, 0), (16400, 0)]
    );
    assert!(door_encounters(&node).is_empty());

    for stage in [
        "debug_deadmines_rhahkzor_falls",
        "debug_deadmines_shredder_ejects_sneed",
        "debug_deadmines_cannon_breaches",
    ] {
        node.assert_call(stage, &[]);
    }
    assert_eq!(
        doors(&node),
        [(13965, 1), (16397, 1), (16399, 0), (16400, 0)]
    );
    assert_eq!(door_encounters(&node), [(0, 2), (4, 2)]);

    node.restart_persistent();
    assert_eq!(
        doors(&node),
        [(13965, 1), (16397, 1), (16399, 0), (16400, 0)]
    );
    assert_eq!(door_encounters(&node), [(0, 2), (4, 2)]);

    node.assert_call("debug_deadmines_sneed_falls", &[]);
    node.assert_call("debug_deadmines_gilnid_falls", &[]);
    assert_eq!(
        doors(&node),
        [(13965, 1), (16397, 1), (16399, 1), (16400, 1)]
    );
    assert_eq!(door_encounters(&node), [(0, 2), (1, 2), (2, 2), (4, 2)]);

    node.restart_persistent();
    assert_eq!(
        doors(&node),
        [(13965, 1), (16397, 1), (16399, 1), (16400, 1)]
    );
    assert_eq!(door_encounters(&node), [(0, 2), (1, 2), (2, 2), (4, 2)]);
}
