//! #88: versioned saves restore the complete authoritative park, and a reloaded park continues
//! exactly like one that never stopped.
use serde_json::Value;
use zoo_core::ZooGame;

const FIXTURE_V1: &str = "tests/fixtures/save-v1.json";
const FIXTURE_V2: &str = "tests/fixtures/save-v2.json";
const FIXTURE_V3: &str = "tests/fixtures/save-v3.json";
const FIXTURE_V4: &str = "tests/fixtures/save-v4.json";
const FIXTURE_V5: &str = "tests/fixtures/save-v5.json";
const FIXTURE_V6: &str = "tests/fixtures/save-v6.json";

/// What the schema 4 → 5 migration (#95) does to a document's habitats: each rectangle becomes
/// its row-major enclosed tiles.
fn rectangles_as_tiles(value: &mut Value) {
    for habitat in value["park"]["habitats"].as_array_mut().unwrap() {
        let habitat = habitat.as_object_mut().unwrap();
        let mut take = |key: &str| habitat.remove(key).unwrap().as_u64().unwrap();
        let (x, y, width, height) = (take("x"), take("y"), take("width"), take("height"));
        habitat.remove("orientation").unwrap();
        let tiles: Vec<Value> = (y..y + height)
            .flat_map(|y| (x..x + width).map(move |x| serde_json::json!({ "x": x, "y": y })))
            .collect();
        habitat.insert("tiles".to_owned(), tiles.into());
    }
}

/// What the schema 5 → 6 migration (#95) does to a document made with the built-in content:
/// fences move into the content, every species gains its minimum fence height and every
/// habitat is fenced with the wooden rail at the old price and upkeep.
fn fences_from_content(value: &mut Value) {
    let built_in = json(&ZooGame::new().content_json());
    let wooden = built_in["fences"][0].clone();
    assert_eq!(wooden["key"], "wooden_rail");
    for habitat in value["park"]["habitats"].as_array_mut().unwrap() {
        if habitat["species"].is_object() {
            let key = habitat["species"]["key"].clone();
            let species = built_in["species"]
                .as_array()
                .unwrap()
                .iter()
                .find(|species| species["key"] == key)
                .unwrap();
            habitat["species"]["minimum_fence_height_cm"] =
                species["minimum_fence_height_cm"].clone();
        }
        habitat
            .as_object_mut()
            .unwrap()
            .insert("fence".to_owned(), wooden.clone());
    }
    value["content"] = built_in;
}

/// Schema 5 parks keep their fence price and upkeep as the wooden rail type and gain the
/// steel mesh; nothing else changes and the park runs on.
#[test]
fn the_schema_v5_fixture_moves_its_fence_into_content() {
    let fixture = std::fs::read_to_string(FIXTURE_V5).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut original = json(&fixture);
    assert_eq!(
        original["content"]["balance"]["construction"]["fence_segment_cents"],
        json(&game.content_json())["fences"][0]["segment_cents"]
    );
    fences_from_content(&mut original);
    original["schema_version"] = 6.into();
    assert_eq!(
        json(&game.save_json()),
        original,
        "migration changes nothing else"
    );
    game.tick(60);
}

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
            mutate(&|v| v["schema_version"] = 7.into()),
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
fn the_schema_v6_fixture_still_loads_unchanged() {
    if std::env::var_os("ZOO_WRITE_SAVE_FIXTURE").is_some() {
        let value = json(&mid_run().save_json());
        std::fs::write(
            FIXTURE_V6,
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
    let fixture = std::fs::read_to_string(FIXTURE_V6).unwrap();
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
    let mut original = json(&fixture);
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}

/// Schema 4 rectangles become enclosure tiles (#95) without changing area, capacity, care,
/// gates or ownership, and the park runs on exactly as before.
#[test]
fn the_schema_v4_fixture_migrates_rectangles_to_enclosure_tiles() {
    let fixture = std::fs::read_to_string(FIXTURE_V4).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut original = json(&fixture);
    let rectangles: Vec<(u64, u64)> = original["park"]["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            let area = h["width"].as_u64().unwrap() * h["height"].as_u64().unwrap();
            (h["id"].as_u64().unwrap(), area)
        })
        .collect();
    assert!(!rectangles.is_empty());
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
    original["schema_version"] = 6.into();
    assert_eq!(
        json(&game.save_json()),
        original,
        "migration changes nothing else"
    );

    let view = json(&game.snapshot_json());
    for (id, area) in rectangles {
        let habitat = view["habitats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["id"] == id)
            .unwrap();
        assert_eq!(habitat["footprint_area"], area);
        assert_eq!(habitat["capacity"], (area / 3).clamp(2, 12));
    }
    game.tick(60);
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
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
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
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
    original["park"].as_object_mut().unwrap().remove("tutorial");
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}
