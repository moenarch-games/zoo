//! #133: habitat care schedules survive the authoritative save/load path (ordered command
//! log replay) with their overrides, due work and missed-work history intact.
use serde_json::Value;
use zoo_core::ZooGame;

fn ok(result: String) {
    let value: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["ok"], true, "{value}");
}

fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

#[test]
fn replay_preserves_schedule_overrides_due_and_missed_work() {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.hire_keeper());
    ok(game.schedule_keeper(1));
    ok(game.hire_keeper());
    ok(game.schedule_keeper(2));
    ok(game.adopt(1, "zebra".to_owned()));
    ok(game.adopt(2, "zebra".to_owned()));
    // No feed stock: feeding jobs stay due and are then missed.
    ok(game.set_care_task(1, "cleaning".to_owned(), 0, 240, 5));
    for _ in 0..5 {
        game.tick(97);
    }

    let view = snapshot(&game);
    let first = &view["habitats"][0];
    let second = &view["habitats"][1];
    let cleaning = |habitat: &Value| {
        habitat["care_schedule"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["category"] == "cleaning")
            .cloned()
            .unwrap()
    };
    assert_eq!(cleaning(first)["interval_minutes"], 240);
    assert_eq!(cleaning(first)["customized"], true);
    assert_eq!(cleaning(second)["interval_minutes"], 360);
    assert_eq!(cleaning(second)["customized"], false);
    let states: Vec<&Value> = first["care_jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| &job["state"])
        .collect();
    assert!(states.iter().any(|state| *state == "missed"), "{states:?}");
    assert!(
        states.iter().any(|state| *state == "completed"),
        "{states:?}"
    );

    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json())
        .expect("complete log replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}

/// #95: replayed command logs rebuild the deterministic default gate, moved gates and the
/// walking keepers' positions and routes exactly.
#[test]
fn replay_rebuilds_service_gates_and_walking_keepers() {
    let mut game = ZooGame::new();
    for x in 5..=7 {
        ok(game.place_path(x, 7));
    }
    for y in 8..=10 {
        ok(game.place_path(7, y));
    }
    ok(game.place_habitat_rect(3, 8, 6, 10));
    let gate = &snapshot(&game)["habitats"][0]["service_gate"];
    assert_eq!((gate["x"].as_u64(), gate["y"].as_u64()), (Some(3), Some(8)));
    assert_eq!(gate["side"], "north");

    ok(game.buy_animal_feed());
    ok(game.hire_keeper());
    ok(game.schedule_keeper(1));
    ok(game.adopt(1, "capybara".to_owned()));
    let preview: Value = serde_json::from_str(&game.evaluate_service_gate(1, 7, 10)).unwrap();
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["segment"]["side"], "east");
    ok(game.set_service_gate(1, 6, 10, "east".to_owned()));
    game.tick(7);

    let view = snapshot(&game);
    assert_eq!(view["habitats"][0]["service_gate"]["access"]["x"], 7);
    assert_eq!(
        view["animal_care_depot"]["keepers"][0]["activity"],
        "to_gate"
    );

    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json())
        .expect("complete log replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}
