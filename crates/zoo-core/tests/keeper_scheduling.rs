//! #134: zone/category/shift keeper scheduling is deterministic and survives the authoritative
//! save/load path (ordered command log replay) mid-assignment with one reservation per job.
use serde_json::Value;
use zoo_core::ZooGame;

fn ok(result: String) {
    let value: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["ok"], true, "{value}");
}

fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

fn park() -> ZooGame {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.buy_animal_feed());
    for keeper_id in 1..=2 {
        ok(game.hire_keeper());
        ok(game.assign_keeper_zone(keeper_id, 1));
    }
    // Keeper #2 only cleans and inspects, from 06:00 to 18:00.
    ok(game.set_keeper_schedule(2, "cleaning,inspection".to_owned(), 360, 1080));
    ok(game.adopt(1, "zebra".to_owned()));
    ok(game.adopt(2, "zebra".to_owned()));
    game
}

/// Every in-progress job has exactly one keeper whose activity points back at it.
fn assert_one_reservation_per_job(view: &Value) {
    let keepers = view["animal_care_depot"]["keepers"].as_array().unwrap();
    let mut in_progress = 0;
    for habitat in view["habitats"].as_array().unwrap() {
        for job in habitat["care_jobs"].as_array().unwrap() {
            if job["state"] != "in_progress" {
                continue;
            }
            in_progress += 1;
            let holders: Vec<&Value> = keepers
                .iter()
                .filter(|keeper| keeper["job_id"] == job["id"])
                .collect();
            assert_eq!(holders.len(), 1, "job {job}");
            assert_eq!(holders[0]["id"], job["keeper_id"]);
        }
    }
    let busy = keepers.iter().filter(|k| !k["job_id"].is_null()).count();
    assert_eq!(busy, in_progress);
}

#[test]
fn identical_commands_produce_identical_assignments_and_replay_mid_assignment() {
    let mut game = park();
    let mut twin = park();
    game.tick(5);
    twin.tick(5);
    assert_eq!(game.snapshot_json(), twin.snapshot_json());

    let view = snapshot(&game);
    let keepers = view["animal_care_depot"]["keepers"].as_array().unwrap();
    assert!(
        keepers.iter().any(|keeper| keeper["activity"] == "to_gate"),
        "a keeper is mid-assignment: {keepers:?}"
    );
    assert_eq!(
        keepers[1]["categories"],
        serde_json::json!(["cleaning", "inspection"])
    );
    assert_one_reservation_per_job(&view);

    let mut replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json())
        .expect("complete log replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());

    for _ in 0..8 {
        game.tick(90);
        replayed.tick(90);
        assert_eq!(replayed.snapshot_json(), game.snapshot_json());
        assert_one_reservation_per_job(&snapshot(&game));
    }
    // Feeding is only keeper #1's work; keeper #2 never takes it.
    for habitat in snapshot(&game)["habitats"].as_array().unwrap() {
        for job in habitat["care_jobs"].as_array().unwrap() {
            if job["category"] == "feeding" && !job["keeper_id"].is_null() {
                assert_eq!(job["keeper_id"], 1, "{job}");
            }
        }
    }
}
