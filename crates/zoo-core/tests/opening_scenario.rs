//! #93 (building on #137): the guided opening scenario teaches a first sustainable zoo. Its
//! checklist follows authoritative park state for any valid layout and step order, recovery
//! hints come from the park's own blockers, and skip/restart/load/replay never duplicate jobs
//! or money.
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
    snapshot(game)["scenario"]["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|objective| objective["done"] == true)
        .map(|objective| objective["id"].as_str().unwrap().to_owned())
        .collect()
}

fn next(game: &ZooGame) -> Value {
    snapshot(game)["tutorial"]["next"].clone()
}

fn problem(game: &ZooGame) -> String {
    snapshot(game)["tutorial"]["problem"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn run_until(game: &mut ZooGame, what: &str, reached: impl Fn(&ZooGame) -> bool) {
    for _ in 0..(48 * 8) {
        if reached(game) {
            return;
        }
        game.tick(15);
    }
    panic!("never reached {what}: {}", snapshot(game)["tutorial"]);
}

fn edit_first_priority(game: &mut ZooGame) {
    let task = snapshot(game)["habitats"][0]["care_schedule"][0].clone();
    ok(game.set_care_task(
        1,
        task["category"].as_str().unwrap().to_owned(),
        task["first_minute_of_day"].as_u64().unwrap() as u32,
        task["interval_minutes"].as_u64().unwrap() as u32,
        (task["priority"].as_u64().unwrap() as u32 % 5) + 1,
    ));
}

#[test]
fn the_opening_teaches_an_operating_zoo_with_recovery_hints() {
    let mut game = ZooGame::new();
    let view = snapshot(&game);
    assert_eq!(view["scenario"]["id"], "first-zoo");
    assert_eq!(view["tutorial"]["status"], "active");
    assert_eq!(next(&game), "build-habitat");
    assert_eq!(view["tutorial"]["tool"], "habitat");
    assert!(
        view["tutorial"]["hint"]
            .as_str()
            .unwrap()
            .contains("Draw habitat")
    );

    // A wrong path: a stub cut off from the entrance. The hint names the tile to connect.
    for x in 8..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(7, 8, 9, 10));
    assert_eq!(next(&game), "connect-gate");
    assert!(
        problem(&game).contains("has no path from the depot"),
        "{}",
        problem(&game)
    );
    for x in 5..=7 {
        ok(game.place_path(x, 7));
    }
    assert_eq!(next(&game), "zone-gate");
    assert!(
        problem(&game).starts_with("No work zone"),
        "{}",
        problem(&game)
    );
    ok(game.create_work_zone(0, 0, 19, 13));
    assert_eq!(next(&game), "staff-zone");
    assert!(
        problem(&game).contains("hire one at the depot"),
        "{}",
        problem(&game)
    );
    ok(game.hire_keeper());
    assert!(
        problem(&game).starts_with("No keeper works in zone #1"),
        "{}",
        problem(&game)
    );
    ok(game.assign_keeper_zone(1, 1));
    assert_eq!(next(&game), "buy-feed");

    // Animals before feed: an unsuitable group and the empty feed store are both explained.
    ok(game.adopt(1, "capybara".to_owned()));
    assert!(problem(&game).contains("out of feed"), "{}", problem(&game));
    ok(game.buy_animal_feed());
    assert_eq!(next(&game), "adopt-group");
    assert!(
        problem(&game).contains("need at least 2 together"),
        "{}",
        problem(&game)
    );
    ok(game.adopt(1, "capybara".to_owned()));
    assert_eq!(next(&game), "add-shelter");
    ok(game.add_shelter(1));

    run_until(&mut game, "care work", |game| next(game) != "see-care-work");
    assert_eq!(next(&game), "edit-schedule");
    edit_first_priority(&mut game);
    run_until(&mut game, "guests", |game| next(game) != "first-guests");
    assert_eq!(next(&game), "open-stand");
    ok(game.place_concession(6, 6, "food".to_owned()));
    run_until(&mut game, "an operating zoo", |game| {
        snapshot(game)["tutorial"]["status"] == "completed"
    });
    let view = snapshot(&game);
    assert!(view["tutorial"]["next"].is_null() && view["tutorial"]["hint"].is_null());
    assert_eq!(done(&game).len(), 13);

    // Saving and loading resumes the finished guide without replaying it.
    let saved = game.save_json();
    let mut resumed = ZooGame::new();
    ok(resumed.load_save(&saved));
    assert_eq!(snapshot(&resumed)["tutorial"]["status"], "completed");
    assert_eq!(done(&resumed).len(), 13);
}

#[test]
fn an_unaffordable_command_explains_how_to_recover() {
    let mut game = ZooGame::new();
    let refused = (0..400).find_map(|_| {
        let result: Value = serde_json::from_str(&game.hire_keeper()).unwrap();
        (result["ok"] == false).then_some(result)
    });
    assert!(refused.is_some(), "hiring never ran out of cash");
    let hint = problem(&game);
    assert!(
        hint.starts_with("Not enough cash") && hint.contains("Undo"),
        "{hint}"
    );
    // A command that works clears the refusal hint.
    ok(game.set_keeper_schedule(1, "feeding".to_owned(), 360, 1080));
    assert!(
        !problem(&game).starts_with("Not enough cash"),
        "{}",
        problem(&game)
    );
}

#[test]
fn alternate_layout_and_out_of_order_steps_are_accepted() {
    let mut game = ZooGame::new();
    // Zone, keeper and feed first, then a vertical habitat somewhere else.
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    assert_eq!(done(&game), ["buy-feed"]);
    for x in 5..=12 {
        ok(game.place_path(x, 7));
    }
    for y in 2..=10 {
        ok(game.place_path(12, y));
    }
    ok(game.place_habitat_rect(13, 3, 15, 7));
    assert_eq!(
        done(&game),
        [
            "build-habitat",
            "connect-gate",
            "zone-gate",
            "staff-zone",
            "buy-feed"
        ]
    );
    assert_eq!(next(&game), "adopt-group");
}

#[test]
fn undoing_work_keeps_latched_steps_and_points_at_the_new_blocker() {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(7, 8, 9, 10));
    assert_eq!(next(&game), "zone-gate");
    ok(game.undo_construction());
    // The steps stay ticked, but the guide points back at rebuilding the habitat.
    assert_eq!(done(&game), ["build-habitat", "connect-gate"]);
    assert_eq!(next(&game), "build-habitat");
    assert_eq!(snapshot(&game)["tutorial"]["tool"], "habitat");
    ok(game.place_habitat_rect(10, 8, 12, 10));
    assert_eq!(next(&game), "zone-gate");
}

#[test]
fn skip_restart_load_and_replay_never_duplicate_jobs_or_money() {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.buy_animal_feed());
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.adopt(1, "capybara".to_owned()));
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
    // Steps the park already satisfies tick again on restart, not re-rewarded.
    assert_eq!(
        after["scenario"]["objectives"],
        before["scenario"]["objectives"]
    );

    // Loading a save keeps the guide's progress and changes nothing else.
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    ok(loaded.load_save(&saved));
    assert_eq!(loaded.save_json(), game.save_json());
    assert_eq!(snapshot(&loaded)["tutorial"], snapshot(&game)["tutorial"]);

    ok(game.skip_tutorial());
    game.tick(60);
    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json()).unwrap();
    assert_eq!(snapshot(&replayed), snapshot(&game));
    assert_eq!(snapshot(&replayed)["tutorial"]["status"], "skipped");
}

#[test]
fn unguided_scenarios_have_no_guide_to_skip_or_restart() {
    let mut game = ZooGame::new();
    ok(game.start_scenario("living-starter"));
    assert_eq!(snapshot(&game)["tutorial"]["status"], "off");
    let skip: Value = serde_json::from_str(&game.skip_tutorial()).unwrap();
    let restart: Value = serde_json::from_str(&game.restart_tutorial()).unwrap();
    assert_eq!(skip["ok"], false);
    assert_eq!(restart["ok"], false);
}
