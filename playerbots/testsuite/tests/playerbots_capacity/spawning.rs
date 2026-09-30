use super::support::Standalone;
use lyracore_shared::{nav, terrain, vmap};
use std::collections::BTreeMap;

fn starting_area(name: &str, start_z: f32, surfaces: &[[[f32; 3]; 4]]) -> Standalone {
    let mut node = Standalone::start(name);
    node.publish_module();
    node.assert_call("claim_operator", &[]);
    node.assert_call("install_guid_range", &["1000000"]);
    node.assert_call("debug_set_nav_enabled", &["true"]);
    node.assert_sql("DELETE FROM game_area WHERE id = 12");
    node.assert_sql("INSERT INTO game_area (id,map_id,parent_area_id,area_bit,flags,exploration_level,faction_group,name) VALUES (12,0,0,0,0,1,0,'Starting area')");
    for class in [1, 5, 8] {
        let key = 256 + class;
        node.assert_sql(&format!(
            "DELETE FROM game_start_position WHERE race_class = {key}"
        ));
        node.assert_sql(&format!("INSERT INTO game_start_position (race_class,race,class,map_id,zone_id,x,y,z,orientation,display_id) VALUES ({key},1,{class},0,12,1200,1200,{start_z},0,49)"));
    }
    let center = terrain::cell_index(1200.0).unwrap();
    let heights = vec!["50"; 145].join(":");
    let mut ground = Vec::new();
    let mut navigation = Vec::new();
    for x in center - 8..=center + 8 {
        for y in center - 8..=center + 8 {
            ground.push(format!("0,{x},{y},0,0,0,12,{heights}"));
            navigation.push(format!("0,{x},{y},50,,"));
        }
    }
    for (reducer, rows) in [
        ("import_terrain_chunks_append", ground),
        ("import_nav_chunks_append", navigation),
    ] {
        for batch in rows.chunks(64) {
            node.assert_call(
                reducer,
                &[&serde_json::to_string(&batch.join(";")).unwrap()],
            );
        }
    }
    let mut cells = BTreeMap::<(u16, u16), Vec<vmap::VmapTri>>::new();
    for surface in surfaces {
        for verts in [
            [surface[0], surface[1], surface[2]],
            [surface[0], surface[2], surface[3]],
        ] {
            let xs: Vec<_> = verts
                .iter()
                .map(|v| terrain::cell_index(v[0]).unwrap())
                .collect();
            let ys: Vec<_> = verts
                .iter()
                .map(|v| terrain::cell_index(v[1]).unwrap())
                .collect();
            for x in *xs.iter().min().unwrap()..=*xs.iter().max().unwrap() {
                for y in *ys.iter().min().unwrap()..=*ys.iter().max().unwrap() {
                    cells.entry((x, y)).or_default().push(vmap::VmapTri {
                        verts,
                        class: vmap::TriClass::Wmo {
                            group_id: 1,
                            mogp_flags: 0,
                        },
                    });
                }
            }
        }
    }
    let mut manifest = blake3::Hasher::new();
    manifest.update(b"lyracore-vmap-manifest-v1");
    let mut rows = Vec::new();
    let mut bytes = 0;
    for ((x, y), triangles) in cells {
        let blob = vmap::encode(&triangles);
        manifest.update(&terrain::cell_key(0, x, y).to_le_bytes());
        manifest.update(&0u32.to_le_bytes());
        manifest.update(&(blob.len() as u32).to_le_bytes());
        manifest.update(&blob);
        bytes += blob.len();
        let hex: String = blob.iter().map(|b| format!("{b:02x}")).collect();
        rows.push(format!("0,0,{x},{y},{hex}"));
    }
    node.assert_call(
        "stage_vmap_generation",
        &[
            "5090910",
            "0",
            &rows.len().to_string(),
            &bytes.to_string(),
            &serde_json::to_string(&manifest.finalize().to_hex().to_string()).unwrap(),
            "\"synthetic-starting-surfaces\"",
            "\"map=0\"",
        ],
    );
    for batch in rows.chunks(64) {
        node.assert_call(
            "append_vmap_generation_chunks",
            &["5090910", &serde_json::to_string(&batch.join(";")).unwrap()],
        );
    }
    node.assert_call("verify_vmap_generation", &["5090910"]);
    node.assert_call("activate_vmap_generation", &["5090910"]);
    node.assert_call("debug_set_vmap_enabled", &["true"]);
    node
}

#[test]
#[ignore = "requires SpacetimeDB and the playerbots Package on a build host"]
fn starting_area_bots_stand_on_the_model_floor_connected_to_the_race_start() {
    let floor = [
        [940.0, 940.0, 60.0],
        [1460.0, 940.0, 60.0],
        [1460.0, 1460.0, 60.0],
        [940.0, 1460.0, 60.0],
    ];
    let node = starting_area("playerbots-start-on-model", 60.0, &[floor]);
    let started = std::time::Instant::now();
    node.assert_call(
        "playerbots_spawn_starting_area",
        &["\"northshire\"", "50", "{\"frozen\":[]}"],
    );
    println!(
        "placement batch count=50 wall_millis={}",
        started.elapsed().as_millis()
    );
    let bots = node.query_rows("SELECT character_guid, home_z FROM pkg_playerbots_bot");
    assert_eq!(bots.len(), 50);
    for bot in bots {
        assert_eq!(bot["home_z"].parse::<f32>().unwrap(), 60.0);
        let body = node.query_rows(&format!(
            "SELECT z, level, xp FROM game_world_entity WHERE guid = {}",
            bot["character_guid"]
        ));
        assert_eq!(body[0]["z"].parse::<f32>().unwrap(), 60.0);
        assert_eq!(body[0]["level"], "1");
        assert_eq!(body[0]["xp"], "0");
    }
}

#[test]
#[ignore = "requires SpacetimeDB and the playerbots Package on a build host"]
fn starting_area_bots_reject_ground_enclosed_by_collision() {
    // The first distributed candidate lies inside this closed room. Its grid bit is open.
    let corners = [
        [1010.0, 1315.0],
        [1070.0, 1315.0],
        [1070.0, 1375.0],
        [1010.0, 1375.0],
    ];
    let walls: Vec<_> = (0..4)
        .map(|i| {
            let a = corners[i];
            let b = corners[(i + 1) % 4];
            [
                [a[0], a[1], 49.0],
                [b[0], b[1], 49.0],
                [b[0], b[1], 70.0],
                [a[0], a[1], 70.0],
            ]
        })
        .collect();
    let node = starting_area("playerbots-start-outside-room", 50.0, &walls);
    node.assert_call(
        "playerbots_spawn_starting_area",
        &["\"northshire\"", "1", "{\"frozen\":[]}"],
    );
    let bots = node.query_rows("SELECT character_guid FROM pkg_playerbots_bot");
    assert_eq!(bots.len(), 1);
    let body = node.query_rows(&format!(
        "SELECT x, y, z FROM game_world_entity WHERE guid = {}",
        bots[0]["character_guid"]
    ));
    let x = body[0]["x"].parse::<f32>().unwrap();
    let y = body[0]["y"].parse::<f32>().unwrap();
    assert!(
        !(1010.0..1070.0).contains(&x) || !(1315.0..1375.0).contains(&y),
        "bot spawned inside the closed room: {body:?}"
    );
    assert_eq!(body[0]["z"].parse::<f32>().unwrap(), 50.0);
    assert!((x - 1200.0).hypot(y - 1200.0) <= 250.01);
}

#[test]
#[ignore = "requires SpacetimeDB and the playerbots Package on a build host"]
fn an_unreachable_class_start_refuses_the_entire_starting_area_batch() {
    let walls = [
        [
            [1299.0, 1199.0, 49.0],
            [1301.0, 1199.0, 49.0],
            [1301.0, 1199.0, 70.0],
            [1299.0, 1199.0, 70.0],
        ],
        [
            [1301.0, 1199.0, 49.0],
            [1301.0, 1201.0, 49.0],
            [1301.0, 1201.0, 70.0],
            [1301.0, 1199.0, 70.0],
        ],
        [
            [1301.0, 1201.0, 49.0],
            [1299.0, 1201.0, 49.0],
            [1299.0, 1201.0, 70.0],
            [1301.0, 1201.0, 70.0],
        ],
        [
            [1299.0, 1201.0, 49.0],
            [1299.0, 1199.0, 49.0],
            [1299.0, 1199.0, 70.0],
            [1299.0, 1201.0, 70.0],
        ],
    ];
    let node = starting_area("playerbots-start-refused-batch", 50.0, &walls);
    node.assert_sql("UPDATE game_start_position SET x = 1300 WHERE race_class = 261");
    let population = || {
        (
            node.query_rows("SELECT guid, name FROM game_character"),
            node.query_rows(
                "SELECT character_guid, home_x, home_y, home_z FROM pkg_playerbots_bot",
            ),
            node.query_rows("SELECT guid FROM game_world_entity WHERE entry = 0"),
        )
    };
    let before = population();
    let output = node.call(
        "playerbots_spawn_starting_area",
        &["\"northshire\"", "2", "{\"frozen\":[]}"],
    );
    assert!(!output.status.success());
    let refusal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        refusal.contains("no reachable imported ground"),
        "{refusal}"
    );
    assert_eq!(population(), before, "refused batch changed the population");
}

#[test]
#[ignore = "requires SpacetimeDB and the playerbots Package on a build host"]
fn starting_area_direct_routes_remain_available_after_searches_are_exhausted() {
    let floor = [
        [940.0, 940.0, 50.0],
        [1460.0, 940.0, 50.0],
        [1460.0, 1460.0, 50.0],
        [940.0, 1460.0, 50.0],
    ];
    let node = starting_area("placement-search-budget", 50.0, &[floor]);
    let center = terrain::cell_index(1200.0).unwrap();
    let mut rows = Vec::new();
    for cx in center - 8..=center + 8 {
        for cy in center - 8..=center + 8 {
            let mut walk = vec![0xff; nav::WALK_BYTES];
            for nx in 0..nav::WALK_DIM {
                for ny in 0..nav::WALK_DIM {
                    let x = (nav::sub_center(cx, nx, nav::WALK_DIM) - 1200.0).abs();
                    let y = (nav::sub_center(cy, ny, nav::WALK_DIM) - 1200.0).abs();
                    if ((99.0..=101.0).contains(&x) && y <= 101.0)
                        || ((99.0..=101.0).contains(&y) && x <= 101.0)
                    {
                        nav::walk_set(&mut walk, nx, ny, false);
                    }
                }
            }
            let hex: String = walk.iter().map(|b| format!("{b:02x}")).collect();
            rows.push(format!("0,{cx},{cy},50,{hex},"));
        }
    }
    for (index, batch) in rows.chunks(64).enumerate() {
        node.assert_call(
            if index == 0 {
                "import_nav_chunks"
            } else {
                "import_nav_chunks_append"
            },
            &[&serde_json::to_string(&batch.join(";")).unwrap()],
        );
    }
    node.assert_call(
        "playerbots_spawn_starting_area",
        &["\"northshire\"", "50", "{\"frozen\":[]}"],
    );
    let bots =
        node.query_rows("SELECT character_guid,home_x,home_y,home_z FROM pkg_playerbots_bot");
    assert_eq!(bots.len(), 50);
    for bot in bots {
        assert!((bot["home_x"].parse::<f32>().unwrap() - 1200.0).abs() < 99.0);
        assert!((bot["home_y"].parse::<f32>().unwrap() - 1200.0).abs() < 99.0);
        assert_eq!(bot["home_z"].parse::<f32>().unwrap(), 50.0);
    }
}
