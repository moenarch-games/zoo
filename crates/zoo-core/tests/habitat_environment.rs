//! #97: terrain, water, elevation, shelter and enrichment are habitat design choices with
//! species requirements, reachability from the service gate and atomic, explained edits.
use serde_json::Value;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) -> Value {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn rejected(result: String) -> Value {
    let value = json(&result);
    assert_eq!(value["ok"], false, "{value}");
    value
}

fn snapshot(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

fn cash(game: &ZooGame) -> i64 {
    snapshot(game)["cash_cents"].as_i64().unwrap()
}

fn habitat(game: &ZooGame, id: u64) -> Value {
    snapshot(game)["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == id)
        .cloned()
        .unwrap()
}

fn part(habitat: &Value, key: &str) -> Value {
    habitat["welfare_parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["key"] == key)
        .cloned()
        .unwrap()
}

fn met(habitat: &Value, key: &str) -> bool {
    part(habitat, key)["met"] == true
}

fn row(x0: u32, x1: u32, y: u32) -> Vec<u32> {
    (x0..=x1).flat_map(|x| [x, y]).collect()
}

/// Two empty 5×5 habitats under the starter path: #1 at x 3..=7 and #2 at x 9..=13, both
/// y 8..=12, staffed and fed. Their service gates face the path on the north row.
fn two_habitats() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(vec![1, 7, 15, 7], 1, "public".to_owned()));
    ok(game.place_habitat_rect(3, 8, 7, 12));
    ok(game.place_habitat_rect(9, 8, 13, 12));
    ok(game.hire_keeper());
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    for id in [1, 2] {
        assert_eq!(
            habitat(&game, id)["service_gate"]["y"],
            8,
            "gate on the north row"
        );
    }
    game
}

/// The penguin layout: sand, a wading tile, a deep south pool, shelter and a play boulder.
fn penguin_layout(game: &mut ZooGame, id: u32, x0: u32) {
    for y in 8..=10 {
        ok(game.paint_habitat_terrain(id, row(x0, x0 + 4, y), "sand".to_owned()));
    }
    ok(game.paint_habitat_terrain(id, vec![x0, 11], "shallow_water".to_owned()));
    ok(game.paint_habitat_terrain(id, row(x0, x0 + 4, 12), "deep_water".to_owned()));
    ok(game.place_habitat_object(id, "basic_shelter".to_owned(), x0 + 4, 11));
    ok(game.place_habitat_object(id, "play_boulder".to_owned(), x0 + 3, 11));
}

/// The zebra layout: open grass with a shelter and a log pile.
fn zebra_layout(game: &mut ZooGame, id: u32, x0: u32) {
    ok(game.place_habitat_object(id, "basic_shelter".to_owned(), x0 + 4, 12));
    ok(game.place_habitat_object(id, "log_pile".to_owned(), x0 + 3, 12));
}

fn adopt(game: &mut ZooGame, id: u32, species: &str, count: u32) {
    for _ in 0..count {
        ok(game.adopt(id, species.to_owned()));
    }
}

const LAYOUT_PARTS: [&str; 5] = [
    "shelter",
    "enrichment",
    "ground_cover",
    "shallow_water",
    "deep_water",
];

#[test]
fn zebras_and_penguins_need_materially_different_layouts() {
    let mut game = two_habitats();
    zebra_layout(&mut game, 1, 3);
    penguin_layout(&mut game, 2, 9);
    adopt(&mut game, 1, "zebra", 3);
    adopt(&mut game, 2, "penguin", 4);
    for id in [1, 2] {
        let habitat = habitat(&game, id);
        for key in LAYOUT_PARTS {
            assert!(
                met(&habitat, key),
                "habitat {id} {key}: {}",
                part(&habitat, key)
            );
        }
        assert!(met(&habitat, "space"), "{}", part(&habitat, "space"));
    }
    let pool = habitat(&game, 2)["environment"]["assessment"].clone();
    assert_eq!(pool["deep_water"], 5);
    assert_eq!(pool["shallow_water"], 1);

    // Swap the species: each layout fails the other's needs.
    let mut swapped = two_habitats();
    zebra_layout(&mut swapped, 1, 3);
    penguin_layout(&mut swapped, 2, 9);
    adopt(&mut swapped, 1, "penguin", 4);
    adopt(&mut swapped, 2, "zebra", 3);
    let penguins_on_grass = habitat(&swapped, 1);
    let zebras_on_sand = habitat(&swapped, 2);
    for key in ["ground_cover", "deep_water", "shallow_water"] {
        assert!(!met(&penguins_on_grass, key), "{key}");
    }
    assert!(!met(&zebras_on_sand, "ground_cover"));
    assert!(
        penguins_on_grass["welfare_target"].as_u64() < habitat(&game, 2)["welfare_target"].as_u64()
    );
    assert!(
        zebras_on_sand["welfare_target"].as_u64() < habitat(&game, 1)["welfare_target"].as_u64()
    );
    let status = penguins_on_grass["care_status"].as_str().unwrap();
    assert!(
        !status.contains("healthy"),
        "the unmet requirement is explained: {status}"
    );
}

#[test]
fn unreachable_resources_provide_no_benefit() {
    let mut game = two_habitats();
    // A two-step plateau in the middle: allowed slope, but too high to climb.
    let preview = json(&game.evaluate_habitat_elevation(1, vec![5, 10], 2));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["cost_cents"], 2 * 600);
    ok(game.set_habitat_elevation(1, vec![5, 10], 2));
    let placed = ok(game.place_habitat_object(1, "log_pile".to_owned(), 5, 10));
    assert!(placed["message"].as_str().unwrap().contains("unreachable"));
    ok(game.place_habitat_object(1, "basic_shelter".to_owned(), 7, 12));
    adopt(&mut game, 1, "zebra", 3);
    let view = habitat(&game, 1);
    let assessment = &view["environment"]["assessment"];
    assert_eq!(assessment["enrichment_capacity"], 0);
    assert_eq!(assessment["unreachable_tiles"], 1);
    assert_eq!(
        assessment["unreachable_objects"].as_array().unwrap().len(),
        1
    );
    assert!(!met(&view, "enrichment"));
    assert!(
        part(&view, "enrichment")["detail"]
            .as_str()
            .unwrap()
            .contains("unreachable")
    );

    // A ramp makes it reachable and the log pile counts.
    ok(game.set_habitat_elevation(1, vec![5, 9], 1));
    let view = habitat(&game, 1);
    assert_eq!(view["environment"]["assessment"]["enrichment_capacity"], 3);
    assert!(met(&view, "enrichment"));
    assert!(view["environment"]["objects"][0]["reachable"] == true);

    // A pool ringed by unclimbable ledges counts for nothing; one ramp opens it up.
    ok(game.paint_habitat_terrain(2, vec![11, 10], "deep_water".to_owned()));
    assert_eq!(
        habitat(&game, 2)["environment"]["assessment"]["deep_water"],
        1
    );
    ok(game.set_habitat_elevation(2, vec![11, 9, 10, 10, 12, 10, 11, 11], 2));
    assert_eq!(
        habitat(&game, 2)["environment"]["assessment"]["deep_water"],
        0
    );
    ok(game.set_habitat_elevation(2, vec![11, 11], 1));
    assert_eq!(
        habitat(&game, 2)["environment"]["assessment"]["deep_water"],
        1
    );
}

#[test]
fn invalid_edits_are_rejected_whole_and_change_nothing() {
    let mut game = two_habitats();
    ok(game.place_habitat_object(1, "basic_shelter".to_owned(), 7, 12));
    adopt(&mut game, 1, "zebra", 3);
    let before = game.save_json();
    let cash_before = cash(&game);
    let gate = habitat(&game, 1)["service_gate"].clone();
    let (gx, gy) = (
        gate["x"].as_u64().unwrap() as u32,
        gate["y"].as_u64().unwrap() as u32,
    );
    let animal = snapshot(&game)["animals"][0].clone();
    let (ax, ay) = (
        animal["x"].as_u64().unwrap() as u32,
        animal["y"].as_u64().unwrap() as u32,
    );

    let cases: Vec<(String, &str)> = vec![
        (
            game.paint_habitat_terrain(1, vec![gx, gy], "shallow_water".to_owned()),
            "dry land",
        ),
        (
            game.paint_habitat_terrain(1, vec![ax, ay], "deep_water".to_owned()),
            "strand",
        ),
        (
            game.paint_habitat_terrain(1, vec![7, 12], "shallow_water".to_owned()),
            "remove it",
        ),
        (game.set_habitat_elevation(1, vec![3, 10], 1), "fence"),
        (game.set_habitat_elevation(1, vec![5, 10], 3), "steep"),
        (game.set_habitat_elevation(1, vec![5, 10], 9), "0 to 4"),
        (
            game.paint_habitat_terrain(1, vec![9, 9], "sand".to_owned()),
            "inside habitat #1",
        ),
        (
            game.paint_habitat_terrain(
                1,
                vec![4, 9, 5, 9, 4, 10, 4, 11, 5, 11, 6, 9, 6, 10, 6, 11, 99, 99],
                "sand".to_owned(),
            ),
            "inside habitat #1",
        ),
        (
            game.place_habitat_object(1, "log_pile".to_owned(), 7, 12),
            "already stands",
        ),
        (
            game.place_habitat_object(1, "log_pile".to_owned(), gx, gy),
            "gate",
        ),
    ];
    for (result, reason) in cases {
        let value = rejected(result);
        assert!(
            value["message"].as_str().unwrap().contains(reason),
            "{reason}: {value}"
        );
        assert_eq!(cash(&game), cash_before);
    }
    let mut after = json(&game.save_json());
    let mut expected = json(&before);
    // Rejections are recorded in the session, never in the park.
    for value in [&mut after, &mut expected] {
        value["park"].as_object_mut().unwrap().remove("alerts");
    }
    assert_eq!(after, expected);

    // Repeating an applied edit is a free no-op.
    ok(game.paint_habitat_terrain(1, vec![4, 9], "sand".to_owned()));
    let cash_mid = cash(&game);
    assert_eq!(cash_before - cash_mid, 400);
    ok(game.paint_habitat_terrain(1, vec![4, 9], "sand".to_owned()));
    assert_eq!(cash(&game), cash_mid);
}

#[test]
fn objects_block_flooding_and_reshapes_keep_layouts_valid() {
    let mut game = two_habitats();
    ok(game.place_habitat_object(1, "basic_shelter".to_owned(), 7, 12));
    // Shrinking the fence past the shelter is refused until it is removed.
    let shrink = [3, 8, 7, 8, 7, 12, 3, 12, 3, 8];
    let preview = json(&game.evaluate_habitat_reshape(1, shrink.to_vec()));
    assert_eq!(preview["ok"], false, "{preview}");
    assert!(
        preview["message"]
            .as_str()
            .unwrap()
            .contains("outside the fence")
    );
    let object = habitat(&game, 1)["environment"]["objects"][0]["id"]
        .as_u64()
        .unwrap();
    ok(game.remove_habitat_object(1, object as u32));
    ok(game.reshape_habitat(1, shrink.to_vec()));
    // An elevated tile that would end up beside the fence also blocks a reshape.
    ok(game.set_habitat_elevation(1, vec![5, 10], 1));
    let narrow = [3, 8, 6, 8, 6, 12, 3, 12, 3, 8];
    let preview = json(&game.evaluate_habitat_reshape(1, narrow.to_vec()));
    assert_eq!(preview["ok"], false, "{preview}");
    assert!(preview["message"].as_str().unwrap().contains("fence"));
}

#[test]
fn environments_survive_save_load_and_replay() {
    let mut game = two_habitats();
    zebra_layout(&mut game, 1, 3);
    penguin_layout(&mut game, 2, 9);
    ok(game.set_habitat_elevation(1, vec![5, 10, 5, 11], 1));
    adopt(&mut game, 1, "zebra", 3);
    adopt(&mut game, 2, "penguin", 4);
    game.tick(240);
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    ok(loaded.load_save(&saved));
    for id in [1, 2] {
        let (a, b) = (habitat(&game, id), habitat(&loaded, id));
        for key in ["environment", "welfare_parts", "welfare_target", "welfare"] {
            assert_eq!(a[key], b[key], "{key}");
        }
    }
    // Animals stay on reachable walkable ground while they roam.
    for animal in snapshot(&game)["animals"].as_array().unwrap() {
        let at = serde_json::json!({ "x": animal["x"], "y": animal["y"] });
        let id = animal["habitat_id"].as_u64().unwrap();
        let reachable = habitat(&game, id)["environment"]["reachable"].clone();
        assert!(reachable.as_array().unwrap().contains(&at), "{animal}");
    }
    let content = ZooGame::new().content_json();
    let replayed = ZooGame::replay(&content, &game.diagnostics_json()).expect("replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}
