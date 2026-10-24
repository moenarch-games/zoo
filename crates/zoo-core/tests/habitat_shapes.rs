//! #95: non-rectangular habitats drawn as closed fence outlines, and transactional reshaping
//! that keeps a populated habitat's animals, care and staff.
use serde_json::Value;
use std::collections::HashSet;
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

fn tiles(habitat: &Value) -> HashSet<(u64, u64)> {
    habitat["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["x"].as_u64().unwrap(), t["y"].as_u64().unwrap()))
        .collect()
}

/// An L under the starter path (y = 7): a 4×2 top at x 3..=6, y 8..=9 and a 2×2 leg below its
/// west half. Corner (x, y) is the north-west corner of tile (x, y).
const L_OUTLINE: [u32; 14] = [3, 8, 7, 8, 7, 10, 5, 10, 5, 12, 3, 12, 3, 8];

/// The default content's prices: base, per tile, per fence segment.
const BASE: i64 = 10_000;
const TILE: i64 = 3_000;
const FENCE: i64 = 1_500;

fn reversed(points: &[u32]) -> Vec<u32> {
    points
        .chunks(2)
        .rev()
        .flat_map(|pair| pair.iter().copied())
        .collect()
}

#[test]
fn l_shaped_outlines_build_the_same_enclosure_in_either_direction() {
    for outline in [L_OUTLINE.to_vec(), reversed(&L_OUTLINE)] {
        let mut game = ZooGame::new();
        let preview = json(&game.evaluate_habitat_outline(outline.clone()));
        assert_eq!(preview["ok"], true, "{preview}");
        assert_eq!(preview["area"], 12);
        assert_eq!(preview["perimeter"], 16);
        assert_eq!(preview["cost_cents"], BASE + 12 * TILE + 16 * FENCE);
        let before = cash(&game);
        ok(game.place_habitat_outline(outline.clone()));
        assert_eq!(before - cash(&game), BASE + 12 * TILE + 16 * FENCE);

        let built = habitat(&game, 1);
        assert_eq!(built["footprint_area"], 12);
        assert_eq!(built["fence_length"], 16);
        assert_eq!(built["capacity"], 4);
        let expected: HashSet<(u64, u64)> = (8..10)
            .flat_map(|y| (3..7).map(move |x| (x, y)))
            .chain((10..12).flat_map(|y| (3..5).map(move |x| (x, y))))
            .collect();
        assert_eq!(tiles(&built), expected);
        // Guests view from outside the fence; the gate opens outside it too.
        let inside = tiles(&built);
        for spot in built["viewing_spots"].as_array().unwrap() {
            let at = (spot["x"].as_u64().unwrap(), spot["y"].as_u64().unwrap());
            assert!(!inside.contains(&at), "viewpoint {at:?} inside the fence");
        }
        let access = &built["service_gate"]["access"];
        assert!(!inside.contains(&(access["x"].as_u64().unwrap(), access["y"].as_u64().unwrap())));
        assert_eq!(built["service_gate"]["reachable"], true);

        // The same outline again is an idempotent no-op.
        let cash_after = cash(&game);
        ok(game.place_habitat_outline(outline));
        assert_eq!(cash(&game), cash_after);
        assert_eq!(snapshot(&game)["habitats"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn broken_or_blocked_outlines_reject_without_spending() {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(vec![5, 7, 15, 7]));
    ok(game.place_habitat_rect(10, 8, 12, 10));
    let before = cash(&game);
    let cases: [(&[u32], &str); 7] = [
        // A gap: the outline never returns to its start.
        (&[3, 8, 7, 8, 7, 12, 3, 12], "Close the fence"),
        // A figure eight crosses itself.
        (&[3, 8, 5, 8, 5, 12, 7, 12, 7, 10, 3, 10, 3, 8], "crosses"),
        // Past the map edge.
        (&[17, 8, 21, 8, 21, 11, 17, 11, 17, 8], "outside the park"),
        (
            &[0, 0, u32::MAX, 0, u32::MAX, 3, 0, 3, 0, 0],
            "outside the park",
        ),
        // Too large.
        (&[0, 0, 20, 0, 20, 14, 0, 14, 0, 0], "limit is 100"),
        // Over another habitat.
        (&[9, 8, 14, 8, 14, 12, 9, 12, 9, 8], "clear grass"),
        // Over the entrance path.
        (&[1, 6, 5, 6, 5, 9, 1, 9, 1, 6], "clear grass"),
    ];
    for (outline, message) in cases {
        let preview = json(&game.evaluate_habitat_outline(outline.to_vec()));
        assert_eq!(preview["ok"], false);
        assert!(
            preview["message"].as_str().unwrap().contains(message),
            "{message}: {preview}"
        );
        rejected(game.place_habitat_outline(outline.to_vec()));
    }
    let huge: Vec<u32> = (0..2_000).flat_map(|i| [i % 7, i % 5]).collect();
    rejected(game.place_habitat_outline(huge));
    assert_eq!(cash(&game), before);
    assert_eq!(snapshot(&game)["habitats"].as_array().unwrap().len(), 1);
}

/// A staffed rectangle (x 3..=6, y 8..=10) with `animals` capybaras.
fn populated(animals: u32) -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.place_habitat_rect(3, 8, 6, 10));
    ok(game.hire_keeper());
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    for _ in 0..animals {
        ok(game.adopt(1, "capybara".to_owned()));
    }
    game
}

/// The rectangle grown by a 2×2 leg below its west half.
const EXPANDED: [u32; 14] = [3, 8, 7, 8, 7, 11, 5, 11, 5, 13, 3, 13, 3, 8];

#[test]
fn expanding_a_populated_habitat_keeps_its_animals_care_and_identity() {
    let mut game = populated(2);
    let before = habitat(&game, 1);
    let preview = json(&game.evaluate_habitat_reshape(1, EXPANDED.to_vec()));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["added_tiles"].as_array().unwrap().len(), 4);
    assert!(preview["removed_tiles"].as_array().unwrap().is_empty());
    // Four new tiles and six new fence segments; the reused fence costs nothing.
    assert_eq!(preview["cost_cents"], 4 * TILE + 6 * FENCE);

    let cash_before = cash(&game);
    ok(game.reshape_habitat(1, EXPANDED.to_vec()));
    assert_eq!(cash_before - cash(&game), 4 * TILE + 6 * FENCE);

    let after = habitat(&game, 1);
    assert_eq!(after["footprint_area"], 16);
    assert_eq!(after["fence_length"], 18);
    for key in [
        "id",
        "species",
        "animals",
        "welfare",
        "food",
        "water",
        "care_schedule",
        "service_gate",
        "work_zone_id",
    ] {
        assert_eq!(after[key], before[key], "{key} survives the reshape");
    }
    for key in ["terrain", "objects"] {
        assert_eq!(
            after["environment"][key], before["environment"][key],
            "environment {key} survives the reshape"
        );
    }
    let inside = tiles(&after);
    for animal in snapshot(&game)["animals"].as_array().unwrap() {
        let at = (animal["x"].as_u64().unwrap(), animal["y"].as_u64().unwrap());
        assert!(inside.contains(&at), "animal at {at:?} left the fence");
    }
    game.tick(120);
    assert_eq!(habitat(&game, 1)["animals"], 2);

    // Undo restores the rectangle and refunds the reshape exactly; redo charges it again.
    let spent = 4 * TILE + 6 * FENCE;
    let cash_now = cash(&game);
    ok(game.undo_construction());
    assert_eq!(cash(&game) - cash_now, spent);
    assert_eq!(habitat(&game, 1)["footprint_area"], 12);
    assert_eq!(habitat(&game, 1)["animals"], 2);
    ok(game.redo_construction());
    assert_eq!(cash(&game), cash_now);
    assert_eq!(habitat(&game, 1)["footprint_area"], 16);
}

#[test]
fn unsafe_shrinks_reject_and_keep_every_animal() {
    let mut game = populated(4);
    let before = snapshot(&game);
    // A 3×3 fence holds three animals; four live here.
    let shrink = [3, 8, 6, 8, 6, 11, 3, 11, 3, 8];
    let preview = json(&game.evaluate_habitat_reshape(1, shrink.to_vec()));
    assert_eq!(preview["ok"], false);
    assert!(
        preview["message"]
            .as_str()
            .unwrap()
            .contains("keeps 4 animals"),
        "{preview}"
    );
    rejected(game.reshape_habitat(1, shrink.to_vec()));
    assert_eq!(snapshot(&game), before);
    // Demolishing stays an explicit, previewed action that names the animals it removes.
    let demolition = json(&game.evaluate_demolition(4, 9));
    assert!(demolition["affected"].to_string().contains("4 animals"));
}

#[test]
fn shrinking_away_from_the_gate_moves_it_onto_the_new_fence() {
    let mut game = populated(1);
    let gate = habitat(&game, 1)["service_gate"].clone();
    assert_eq!(
        (gate["x"].clone(), gate["side"].clone()),
        (3.into(), "north".into())
    );
    // Drop the west column, where the gate stands.
    let shrink = [4, 8, 7, 8, 7, 11, 4, 11, 4, 8];
    let preview = json(&game.evaluate_habitat_reshape(1, shrink.to_vec()));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["cost_cents"], FENCE * 3, "only the new west fence");
    assert!(
        preview["warnings"]
            .to_string()
            .contains("service gate moves")
    );
    ok(game.reshape_habitat(1, shrink.to_vec()));
    let after = habitat(&game, 1);
    assert_eq!(after["footprint_area"], 9);
    assert_eq!(after["service_gate"]["reachable"], true);
    let fence = after["fence_segments"].as_array().unwrap();
    let gate = &after["service_gate"];
    assert!(fence.iter().any(|segment| {
        segment["x"] == gate["x"] && segment["y"] == gate["y"] && segment["side"] == gate["side"]
    }));
    // The freed column is grass again and buildable.
    ok(game.place_path(3, 9));
}

#[test]
fn reshapes_replay_exactly_from_the_session_log() {
    let mut game = populated(2);
    ok(game.build_path_stroke(vec![5, 7, 15, 7]));
    ok(game.place_habitat_outline(vec![10, 8, 14, 8, 14, 10, 12, 10, 12, 12, 10, 12, 10, 8]));
    ok(game.reshape_habitat(1, EXPANDED.to_vec()));
    game.tick(30);
    ok(game.undo_construction());
    game.tick(30);
    let content = ZooGame::new().content_json();
    let replayed = ZooGame::replay(&content, &game.diagnostics_json()).expect("replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}
