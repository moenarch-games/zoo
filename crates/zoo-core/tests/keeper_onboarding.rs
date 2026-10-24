//! #137: the keeper-training checklist follows authoritative park state for any valid
//! layout and step order, and skip/restart/replay never duplicate jobs or money.
use serde_json::Value;
use zoo_core::ZooGame;

fn ok(result: String) {
    let value: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["ok"], true, "{value}");
}

fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

fn done(game: &ZooGame) -> Vec<String> {
    snapshot(game)["tutorial"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|step| step["done"] == true)
        .map(|step| step["key"].as_str().unwrap().to_owned())
        .collect()
}

fn next(game: &ZooGame) -> Value {
    snapshot(game)["tutorial"]["next"].clone()
}

fn run_until_completed_job(game: &mut ZooGame) {
    for _ in 0..(48 * 4) {
        game.tick(15);
        if snapshot(game)["tutorial"]["status"] == "completed" {
            return;
        }
    }
    panic!(
        "no keeper completed a care job: {}",
        snapshot(game)["tutorial"]
    );
}

#[test]
fn checklist_teaches_zone_keeper_schedule_and_completion_in_order() {
    let mut game = ZooGame::new();
    assert_eq!(snapshot(&game)["tutorial"]["status"], "active");
    assert_eq!(next(&game), "build_habitat");

    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(7, 8, 9, 10));
    assert_eq!(done(&game), ["build_habitat"]);
    assert_eq!(next(&game), "zone_gate");
    // The visible staffing problem is the park's own: the gate has no work zone.
    let problem = snapshot(&game)["tutorial"]["problem"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(problem.starts_with("No work zone"), "{problem}");

    ok(game.create_work_zone(0, 0, 19, 13));
    assert_eq!(next(&game), "assign_keeper");
    let problem = snapshot(&game)["tutorial"]["problem"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        problem.starts_with("No keeper works in zone #1"),
        "{problem}"
    );

    ok(game.buy_animal_feed());
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    assert_eq!(next(&game), "stock_habitat");
    ok(game.adopt(1, "zebra".to_owned()));
    assert_eq!(snapshot(&game)["tutorial"]["habitat_id"], 1);

    // The species routine generates jobs without the player building a schedule.
    for _ in 0..96 {
        if next(&game) != "job_generated" {
            break;
        }
        game.tick(15);
    }
    assert_eq!(next(&game), "edit_schedule");

    let task = snapshot(&game)["habitats"][0]["care_schedule"][0].clone();
    ok(game.set_care_task(
        1,
        task["category"].as_str().unwrap().to_owned(),
        task["first_minute_of_day"].as_u64().unwrap() as u32,
        task["interval_minutes"].as_u64().unwrap() as u32,
        (task["priority"].as_u64().unwrap() as u32 % 5) + 1,
    ));
    run_until_completed_job(&mut game);
    assert_eq!(done(&game).len(), 7);
    assert!(snapshot(&game)["tutorial"]["next"].is_null());
}

#[test]
fn alternate_layout_and_out_of_order_steps_are_accepted() {
    let mut game = ZooGame::new();
    // Zone and keeper first, then a vertical habitat at a different place.
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    assert!(done(&game).is_empty());
    for y in 2..=10 {
        ok(game.place_path(12, y));
    }
    ok(game.place_habitat_rect(13, 3, 15, 7));
    assert_eq!(done(&game), ["build_habitat", "zone_gate", "assign_keeper"]);
}

#[test]
fn skip_restart_and_replay_never_duplicate_jobs_or_money() {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.buy_animal_feed());
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.adopt(1, "zebra".to_owned()));
    game.tick(120);

    let park = |view: &Value| {
        let mut view = view.clone();
        view.as_object_mut().unwrap().remove("tutorial");
        view
    };
    let before = snapshot(&game);
    ok(game.skip_tutorial());
    assert_eq!(snapshot(&game)["tutorial"]["status"], "skipped");
    assert!(snapshot(&game)["tutorial"]["next"].is_null());
    ok(game.restart_tutorial());
    ok(game.restart_tutorial());
    let after = snapshot(&game);
    assert_eq!(park(&before), park(&after));
    assert_eq!(after["tutorial"]["status"], "active");
    // Steps the park already satisfies are ticked again on restart, not re-rewarded.
    assert_eq!(after["tutorial"]["steps"], before["tutorial"]["steps"]);

    ok(game.skip_tutorial());
    game.tick(60);
    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json()).unwrap();
    assert_eq!(snapshot(&replayed), snapshot(&game));
    assert_eq!(snapshot(&replayed)["tutorial"]["status"], "skipped");
}
