use std::time::Instant;

use serde_json::{Value, json};
use zoo_core::ZooGame;

const RUNS: usize = 3;
const SIMULATED_MINUTES: u32 = 240;

fn require_ok(result: String) {
    let value: Value = serde_json::from_str(&result).expect("action result must be JSON");
    assert_eq!(
        value.get("ok").and_then(Value::as_bool),
        Some(true),
        "{value}"
    );
}

fn configured_game() -> ZooGame {
    let mut game = ZooGame::new();

    require_ok(game.place_habitat(3, 8, 0));
    require_ok(game.buy_animal_feed());
    require_ok(game.hire_keeper());
    require_ok(game.create_work_zone(0, 0, 19, 13));
    require_ok(game.assign_keeper_zone(1, 1));
    require_ok(game.add_shelter(1));
    require_ok(game.adopt(1, "capybara".to_owned()));
    require_ok(game.adopt(1, "capybara".to_owned()));

    require_ok(game.place_concession(1, 6, "drink".to_owned()));
    require_ok(game.place_concession(3, 6, "food".to_owned()));
    require_ok(game.hire_janitor());
    require_ok(game.hire_mechanic());

    game
}

fn main() {
    let mut elapsed_ns = Vec::with_capacity(RUNS);
    let mut expected_snapshot: Option<String> = None;

    for _ in 0..RUNS {
        let mut game = configured_game();
        let started = Instant::now();
        game.tick(SIMULATED_MINUTES);
        elapsed_ns.push(started.elapsed().as_nanos());

        let snapshot = game.snapshot_json();
        if let Some(expected) = &expected_snapshot {
            assert_eq!(
                &snapshot, expected,
                "zoo performance journey became nondeterministic"
            );
        } else {
            expected_snapshot = Some(snapshot);
        }
    }

    elapsed_ns.sort_unstable();
    let snapshot = expected_snapshot.expect("at least one run");
    let value: Value = serde_json::from_str(&snapshot).expect("snapshot must be JSON");
    let guest_count = value
        .get("guest_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let habitat_count = value
        .get("habitats")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let concession_count = value
        .get("concessions")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schemaVersion": 1,
            "suite": "zoo/management-journey-smoke",
            "scenario": "staffed-habitat-operations",
            "simulatedMinutes": SIMULATED_MINUTES,
            "runs": RUNS,
            "elapsedNs": elapsed_ns,
            "medianElapsedNs": elapsed_ns[RUNS / 2],
            "snapshotBytes": snapshot.len(),
            "guestCount": guest_count,
            "habitatCount": habitat_count,
            "concessionCount": concession_count,
            "deterministic": true,
            "timing": "advisory-shared-runner"
        }))
        .expect("benchmark report serialization must succeed")
    );
}
