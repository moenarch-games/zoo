//! #88: versioned saves restore the complete authoritative park, and a reloaded park continues
//! exactly like one that never stopped.
use serde_json::Value;
use zoo_core::ZooGame;

const FIXTURE_V1: &str = "tests/fixtures/save-v1.json";
const FIXTURE_V2: &str = "tests/fixtures/save-v2.json";
const FIXTURE_V3: &str = "tests/fixtures/save-v3.json";
const FIXTURE_V4: &str = "tests/fixtures/save-v4.json";

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
}

/// A staffed two-habitat zoo with food and drink stands, a janitor and a mechanic.
fn park() -> ZooGame {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.place_concession(6, 6, "food".to_owned()));
    ok(game.place_concession(8, 6, "drink".to_owned()));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.buy_animal_feed());
    for keeper_id in 1..=2 {
        ok(game.hire_keeper());
        ok(game.assign_keeper_zone(keeper_id, 1));
    }
    ok(game.hire_janitor());
    ok(game.hire_mechanic());
    ok(game.set_keeper_schedule(2, "cleaning,inspection".to_owned(), 360, 1080));
    ok(game.adopt(1, "zebra".to_owned()));
    ok(game.adopt(2, "capybara".to_owned()));
    game
}

/// Advances to a mid-run point where guests walk, care is under way and stands are worn.
fn mid_run() -> ZooGame {
    let mut game = park();
    for _ in 0..13 {
        game.tick(37);
    }
    let view = json(&game.snapshot_json());
    let guests = view["guests"].as_array().unwrap();
    assert!(
        guests.iter().any(|guest| guest["state"] != "viewing"),
        "guests are traveling"
    );
    let jobs: Vec<&Value> = view["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|habitat| habitat["care_jobs"].as_array().unwrap())
        .collect();
    assert!(!jobs.is_empty(), "care jobs exist");
    assert!(
        view["concessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stand| stand["condition"].as_u64().unwrap() < 100),
        "facility wear"
    );
    game
}

/// Identical commands and ticks after the save point, crossing into the next finance day.
fn continue_run(game: &mut ZooGame) {
    ok(game.refill_water(2));
    for _ in 0..30 {
        game.tick(29);
    }
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(3, 1));
    ok(game.place_concession(10, 6, "food".to_owned()));
    for _ in 0..20 {
        game.tick(31);
    }
}

#[test]
fn a_reloaded_park_continues_exactly_like_an_uninterrupted_one() {
    let mut continuous = mid_run();
    let day_at_save = json(&continuous.snapshot_json())["day"].clone();

    let saved = mid_run().save_json();
    let mut resumed = ZooGame::new();
    ok(resumed.load_save(&saved));
    assert_eq!(resumed.save_json(), saved, "load then save is lossless");
    // Construction undo history is session state (#92): a load starts with none.
    let without_history = |game: &ZooGame| {
        let mut view = json(&game.snapshot_json());
        let history = view
            .as_object_mut()
            .unwrap()
            .remove("construction")
            .unwrap();
        (view, history)
    };
    let (resumed_view, resumed_history) = without_history(&resumed);
    let (continuous_view, continuous_history) = without_history(&continuous);
    assert_eq!(resumed_view, continuous_view);
    assert_eq!(resumed_history["undo_depth"], 0);
    assert_ne!(continuous_history["undo_depth"], 0);

    continue_run(&mut continuous);
    continue_run(&mut resumed);
    assert_ne!(json(&continuous.snapshot_json())["day"], day_at_save);
    assert_eq!(resumed.save_json(), continuous.save_json());
    assert_eq!(resumed.snapshot_json(), continuous.snapshot_json());
}

#[test]
fn rejected_saves_leave_the_current_park_untouched() {
    let saved = mid_run().save_json();
    let mut game = park();
    game.tick(45);
    let before = game.save_json();
    let snapshot_before = game.snapshot_json();

    let mutate = |edit: &dyn Fn(&mut Value)| {
        let mut value = json(&saved);
        edit(&mut value);
        value.to_string()
    };
    let cases = [
        ("malformed", saved[..saved.len() / 2].to_owned()),
        ("malformed", "not json".to_owned()),
        ("too_large", " ".repeat(4 * 1024 * 1024 + 1)),
        (
            "unsupported_format",
            mutate(&|v| v["format"] = "zoo-diagnostics/1".into()),
        ),
        (
            "unsupported_version",
            mutate(&|v| v["schema_version"] = 5.into()),
        ),
        (
            "invalid_save",
            mutate(&|v| v["park"]["cash_cents"] = "lots".into()),
        ),
        (
            "invalid_save",
            mutate(&|v| v["park"]["surprise"] = true.into()),
        ),
        (
            "invalid_content",
            mutate(&|v| v["content"]["species"][0]["asset_ref"] = "missing".into()),
        ),
        (
            "invalid_park",
            mutate(&|v| {
                let keeper = v["park"]["keepers"][0].clone();
                v["park"]["keepers"].as_array_mut().unwrap().push(keeper);
            }),
        ),
        (
            "invalid_park",
            mutate(&|v| v["park"]["guests"][0]["target_habitat"] = 99.into()),
        ),
        (
            "invalid_park",
            mutate(&|v| v["park"]["habitats"][0]["welfare"] = 101.into()),
        ),
        (
            "invalid_park",
            mutate(&|v| v["park"]["next_keeper_id"] = 1.into()),
        ),
    ];
    for (code, document) in cases {
        let result = json(&game.load_save(&document));
        assert_eq!(result["ok"], false, "{code}: {result}");
        assert_eq!(result["code"], code, "{result}");
        assert_eq!(game.save_json(), before);
        assert_eq!(game.snapshot_json(), snapshot_before);
        assert_eq!(game.session_status(), "running");
    }
}

#[test]
fn repeated_imports_replace_rather_than_accumulate() {
    let saved = mid_run().save_json();
    let mut game = ZooGame::new();
    ok(game.load_save(&saved));
    ok(game.load_save(&saved));
    ok(game.load_save(&saved));
    assert_eq!(game.save_json(), saved);
}

#[test]
fn the_schema_v4_fixture_still_loads_unchanged() {
    if std::env::var_os("ZOO_WRITE_SAVE_FIXTURE").is_some() {
        let value = json(&mid_run().save_json());
        std::fs::write(
            FIXTURE_V4,
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
    let fixture = std::fs::read_to_string(FIXTURE_V4).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    assert_eq!(json(&game.save_json()), json(&fixture));
    let diagnostics = json(&game.diagnostics_json());
    assert_eq!(diagnostics["start"]["scenario"], "save");
    game.tick(60);
}

/// Schema 3 parks gain alerts, re-read from the park as it loads; nothing else changes.
#[test]
fn the_schema_v3_fixture_migrates_by_reading_its_alerts() {
    let fixture = std::fs::read_to_string(FIXTURE_V3).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    let alerts = saved["park"]
        .as_object_mut()
        .unwrap()
        .remove("alerts")
        .unwrap();
    let minute = json(&game.snapshot_json());
    let now =
        (minute["day"].as_u64().unwrap() - 1) * 1440 + minute["minute_of_day"].as_u64().unwrap();
    for alert in alerts.as_array().unwrap() {
        assert_eq!(alert["first_minute"].as_u64(), Some(now), "{alert}");
    }
    saved["schema_version"] = 3.into();
    assert_eq!(saved, json(&fixture), "migration changes nothing else");
}

/// Schema 2 keeper-training parks continue as the guided opening scenario: same park, same
/// guide status, the opening objectives re-checked from park state on the next command.
#[test]
fn the_schema_v2_fixture_migrates_keeper_training_to_the_opening_guide() {
    let fixture = std::fs::read_to_string(FIXTURE_V2).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    let park = saved["park"].as_object_mut().unwrap();
    assert_eq!(park["scenario"]["id"], "first-zoo");
    assert_eq!(park["tutorial"], serde_json::json!({ "status": "active" }));
    assert_eq!(park["objectives"].as_array().unwrap().len(), 13);
    let mut original = json(&fixture);
    for value in [&mut saved, &mut original] {
        let park = value["park"].as_object_mut().unwrap();
        for key in ["tutorial", "objectives", "alerts"] {
            park.remove(key);
        }
        park["scenario"].as_object_mut().unwrap().remove("id");
        value["schema_version"] = 0.into();
    }
    assert_eq!(saved, original, "migration changes nothing else");

    ok(game.buy_animal_feed());
    let view = json(&game.snapshot_json());
    let done = view["scenario"]["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|objective| objective["done"] == true)
        .count();
    assert!(done >= 5, "the fixture's built zoo ticks its steps: {view}");
    assert_eq!(view["tutorial"]["status"], "active");
}

/// Schema 1 parks were all the fixed empty starter layout; migration records exactly that.
#[test]
fn the_schema_v1_fixture_migrates_to_the_opening_scenario() {
    let fixture = std::fs::read_to_string(FIXTURE_V1).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    let park = saved["park"].as_object_mut().unwrap();
    assert_eq!(park["scenario"]["id"], "first-zoo");
    assert_eq!(park["scenario"]["digest"], Value::Null);
    assert_eq!(park["entrance"], serde_json::json!({ "x": 0, "y": 7 }));
    assert_eq!(park["depot"], serde_json::json!({ "x": 2, "y": 6 }));
    for key in [
        "entrance",
        "depot",
        "scenario",
        "unlocks",
        "objectives",
        "tutorial",
        "alerts",
    ] {
        park.remove(key);
    }
    saved["schema_version"] = 1.into();
    let mut original = json(&fixture);
    original["park"].as_object_mut().unwrap().remove("tutorial");
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}
