//! #94: structured alerts point at the cause of a park problem and disappear once the player
//! fixes it; overlays and the daily summary are read from the same authoritative state.
use serde_json::Value;
use zoo_core::ZooGame;

fn ok(result: String) {
    let value: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["ok"], true, "{value}");
}

fn view(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

fn alert(game: &ZooGame, code: &str) -> Option<Value> {
    view(game)["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|alert| alert["code"] == code)
        .cloned()
}

fn overlay(game: &ZooGame, kind: &str) -> Value {
    serde_json::from_str(&game.overlay_json(kind)).unwrap()
}

/// The empty starter's entrance is at (0, 7) and the depot at (2, 6). A path along row 7, a
/// zoned, keeper-staffed habitat below it, and capybaras in it.
fn staffed_park() -> ZooGame {
    let mut game = ZooGame::new();
    for x in 1..=9 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 7, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.adopt(1, "capybara".to_owned()));
    game
}

#[test]
fn an_empty_feed_depot_alert_points_at_the_depot_and_clears_when_feed_is_bought() {
    let mut game = staffed_park();
    let empty = alert(&game, "feed_depot_empty").expect("stocked park with no feed");
    assert_eq!(empty["severity"], "critical");
    assert_eq!(empty["subject"], serde_json::json!({ "kind": "depot" }));
    assert_eq!(empty["action"], "buy_feed");
    let depot = &view(&game)["animal_care_depot"];
    assert_eq!(empty["focus"]["x"], depot["x"]);
    assert_eq!(empty["focus"]["y"], depot["y"]);

    // A recurring condition stays one alert: first occurrence kept, last occurrence moves.
    game.tick(30);
    let later = alert(&game, "feed_depot_empty").unwrap();
    assert_eq!(later["first_minute"], empty["first_minute"]);
    assert_eq!(
        later["last_minute"].as_u64().unwrap(),
        empty["last_minute"].as_u64().unwrap() + 30
    );
    let count = view(&game)["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|alert| alert["code"] == "feed_depot_empty")
        .count();
    assert_eq!(count, 1);

    // Paused: the command result alone clears it.
    ok(game.buy_animal_feed());
    assert!(alert(&game, "feed_depot_empty").is_none());
}

#[test]
fn an_isolated_destination_alert_clears_when_a_path_connects_it() {
    let mut game = staffed_park();
    ok(game.buy_animal_feed());
    // A second stocked habitat beside a path stub the entrance does not reach.
    for y in 2..=5 {
        ok(game.place_path(12, y));
    }
    ok(game.place_habitat_rect(13, 2, 16, 5));
    ok(game.adopt(2, "capybara".to_owned()));
    ok(game.adopt(2, "capybara".to_owned()));
    let isolated = view(&game)["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|alert| {
            alert["code"] == "destination_isolated"
                && alert["subject"] == serde_json::json!({ "kind": "habitat", "id": 2 })
        })
        .cloned()
        .expect("habitat 2 cannot be seen by guests");
    assert_eq!(isolated["action"], "build_path");
    let access = overlay(&game, "access");
    assert!(
        access["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["subject"] == isolated["subject"] && item["level"] == "bad"),
        "{access}"
    );

    for x in 10..=12 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_path(12, 6));
    let still = view(&game)["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|alert| {
            alert["code"] == "destination_isolated"
                && alert["subject"] == serde_json::json!({ "kind": "habitat", "id": 2 })
        });
    assert!(!still, "a path along its west fence connects it");
}

#[test]
fn an_unserviced_facility_alert_suggests_a_mechanic_and_clears_once_one_is_hired() {
    let mut game = staffed_park();
    ok(game.buy_animal_feed());
    ok(game.place_concession(9, 6, "food".to_owned()));
    let mut ticks = 0;
    while alert(&game, "facility_unserviced").is_none() && alert(&game, "facility_failed").is_none()
    {
        game.tick(60);
        ticks += 1;
        assert!(ticks < 200, "the stand wears out");
    }
    let unserviced = alert(&game, "facility_unserviced")
        .or_else(|| alert(&game, "facility_failed"))
        .unwrap();
    assert_eq!(unserviced["action"], "hire_mechanic");
    assert_eq!(unserviced["subject"]["kind"], "concession");
    let facilities = overlay(&game, "facilities");
    assert!(facilities["warn"].as_u64().unwrap() + facilities["bad"].as_u64().unwrap() > 0);

    ok(game.hire_mechanic());
    assert!(alert(&game, "facility_unserviced").is_none());
    let mut ticks = 0;
    while alert(&game, "facility_failed").is_some() {
        game.tick(10);
        ticks += 1;
        assert!(ticks < 100, "the mechanic repairs the stand");
    }
}

#[test]
fn demolishing_an_alerted_object_leaves_no_stale_alert() {
    let mut game = staffed_park();
    for y in 2..=5 {
        ok(game.place_path(12, y));
    }
    ok(game.place_habitat_rect(13, 2, 16, 5));
    ok(game.adopt(2, "capybara".to_owned()));
    let mentions_two = |game: &ZooGame| {
        view(game)["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alert| alert["subject"] == serde_json::json!({ "kind": "habitat", "id": 2 }))
    };
    assert!(mentions_two(&game));
    ok(game.bulldoze(14, 3));
    assert!(!mentions_two(&game));
}

#[test]
fn the_daily_summary_reconciles_with_the_ledger_and_its_entities() {
    let mut game = staffed_park();
    ok(game.buy_animal_feed());
    game.tick(240);
    let view = view(&game);
    let summary = &view["summary"];
    let today = &view["finance"]["current_day"];
    assert_eq!(summary["profit_cents"], today["profit_cents"]);
    assert_eq!(summary["income_cents"], today["income_cents"]);
    assert_eq!(
        summary["admissions_income_cents"],
        today["breakdown"]["admissions_income_cents"]
    );
    assert_eq!(
        summary["admissions"].as_i64().unwrap()
            * summary["admission_price_cents"].as_i64().unwrap(),
        summary["admissions_income_cents"].as_i64().unwrap()
    );
    let complaint_total: u64 = summary["complaint_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["guest_ids"].as_array().unwrap().len() as u64)
        .sum();
    assert_eq!(summary["complaints"].as_u64().unwrap(), complaint_total);
    let complaints = &view["complaints"];
    let snapshot_total: u64 = ["hungry", "thirsty", "tired", "poor_value"]
        .iter()
        .map(|key| complaints[key].as_u64().unwrap())
        .sum();
    assert_eq!(complaint_total, snapshot_total);
    let open_jobs = view["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|habitat| habitat["care_jobs"].as_array().unwrap())
        .filter(|job| job["state"] == "due" || job["state"] == "in_progress")
        .count() as u64;
    assert_eq!(
        summary["unfinished"].as_u64().unwrap(),
        open_jobs
            + view["litter"].as_array().unwrap().len() as u64
            + view["maintenance"].as_array().unwrap().len() as u64
    );
    assert_eq!(
        summary["welfare"], view["habitats"][0]["welfare"],
        "one stocked habitat"
    );

    // Saved and reloaded, the summary and alerts are the same.
    let saved = game.save_json();
    let mut reloaded = ZooGame::new();
    ok(reloaded.load_save(&saved));
    let again = self::view(&reloaded);
    assert_eq!(again["summary"], view["summary"]);
    assert_eq!(again["alerts"], view["alerts"]);
}

#[test]
fn every_overlay_kind_is_available_and_unknown_kinds_are_null() {
    let game = staffed_park();
    for kind in [
        "access",
        "welfare",
        "staff",
        "cleanliness",
        "facilities",
        "viewing",
    ] {
        let overlay = overlay(&game, kind);
        assert_eq!(overlay["kind"], kind);
        assert_eq!(overlay["legend"].as_array().unwrap().len(), 3);
    }
    assert_eq!(game.overlay_json("nonsense"), "null");
}

/// The current guest flow rarely overfills a viewpoint by itself (arrivals are spaced out and
/// routing prefers free spots), so the crowd is staged in a saved park: four guests watching
/// habitat #2's one capybara from a spot that holds two. Browser acceptance stages it the same
/// way.
#[test]
fn a_crowded_viewpoint_alert_points_at_the_viewpoint_and_clears_when_the_crowd_moves_on() {
    let mut document: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/save-v4.json").unwrap())
            .unwrap();
    let park = &mut document["park"];
    let guests = park["guests"].as_array_mut().unwrap();
    for guest in guests.iter_mut().take(4) {
        guest["state"] = "viewing".into();
        guest["target_habitat"] = 2.into();
        guest["x"] = 8.into();
        guest["y"] = 7.into();
        guest["route"] = serde_json::json!([]);
        guest["route_index"] = 0.into();
        guest["viewing_minutes"] = 20.into();
        guest["arrival_steps"] = 0.into();
        guest["visited_habitats"] = serde_json::json!([2]);
    }
    let mut game = ZooGame::new();
    ok(game.load_save(&document.to_string()));
    let crowded = alert(&game, "viewpoint_crowded").expect("four viewers at a spot for two");
    assert_eq!(
        crowded["subject"],
        serde_json::json!({ "kind": "habitat", "id": 2 })
    );
    assert_eq!(crowded["focus"], serde_json::json!({ "x": 8, "y": 7 }));
    assert_eq!(crowded["action"], "build_path");
    let viewing = overlay(&game, "viewing");
    assert!(
        viewing["cells"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cell| cell["level"] == "bad" && cell["x"] == 8 && cell["y"] == 7),
        "{viewing}"
    );
    let mut ticks = 0;
    while alert(&game, "viewpoint_crowded").is_some() {
        game.tick(1);
        ticks += 1;
        assert!(ticks <= 21, "the crowd finishes viewing and moves on");
    }
}
