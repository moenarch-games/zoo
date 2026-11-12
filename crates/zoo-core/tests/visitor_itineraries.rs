//! Visitor groups and itineraries (#103) through the public game seam: save/load continuation
//! of plans, reservations and choices, advance-size independence, and route/decision work.
use serde_json::Value;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn accepted(result: String) {
    let result = json(&result);
    assert_eq!(result["ok"], true, "{result}");
}

/// Three habitats along one path with toilets and stands between them.
fn park() -> ZooGame {
    let mut game = ZooGame::new();
    accepted(game.skip_tutorial());
    accepted(game.build_path_stroke(vec![1, 7, 18, 7], 1, "public".to_owned()));
    accepted(game.create_work_zone(0, 0, 19, 13));
    accepted(game.buy_animal_feed());
    for (index, (rect, species)) in [
        ((3, 8, 5, 10), "capybara"),
        ((9, 8, 11, 10), "zebra"),
        ((15, 4, 17, 6), "giraffe"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = index as u32 + 1;
        accepted(game.place_habitat_rect(rect.0, rect.1, rect.2, rect.3));
        accepted(game.hire_keeper());
        accepted(game.assign_keeper_zone(id, 1));
        if species == "giraffe" {
            accepted(game.upgrade_fence(id, "steel_mesh".to_owned()));
        }
        accepted(game.adopt(id, species.to_owned()));
    }
    for (x, kind) in [(7, "toilet"), (8, "drink"), (12, "food"), (13, "toilet")] {
        accepted(game.place_concession(x, 6, kind.to_owned()));
    }
    game
}

fn guests(view: &Value) -> &Vec<Value> {
    view["guests"].as_array().unwrap()
}

#[test]
fn save_and_load_continue_plans_groups_and_reservations_identically() {
    let mut game = park();
    // Run until a group waits for a member and another guest holds a facility slot.
    let mut found = false;
    for _ in 0..900 {
        game.tick(1);
        let view = json(&game.snapshot_json());
        let waiting = guests(&view)
            .iter()
            .any(|guest| guest["group"]["waiting_for_group"] == true);
        let errand = guests(&view)
            .iter()
            .any(|guest| !guest["comfort"]["facility_id"].is_null());
        let planned = guests(&view)
            .iter()
            .any(|guest| guest["group"]["plan"] != "" && guest["state"] == "walking_to_habitat");
        if waiting && errand && planned {
            found = true;
            break;
        }
    }
    assert!(found, "a group waited while another guest ran an errand");
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    accepted(loaded.load_save(&saved));
    assert_eq!(loaded.save_json(), saved);
    for _ in 0..12 {
        game.tick(10);
        loaded.tick(10);
        assert_eq!(game.save_json(), loaded.save_json());
    }
    let view = json(&loaded.snapshot_json());
    assert!(
        view["visits"]["total"]
            .as_object()
            .unwrap()
            .values()
            .any(|count| count.as_u64() > Some(0))
    );
}

#[test]
fn decisions_do_not_depend_on_how_the_park_is_advanced() {
    let mut stepped = park();
    let mut jumped = park();
    for _ in 0..480 {
        stepped.tick(1);
    }
    jumped.tick(240);
    jumped.tick(240);
    assert_eq!(stepped.save_json(), jumped.save_json());
    // Reading snapshots, as a renderer does every frame, changes no decision or work counter.
    let work = |game: &ZooGame| json(&game.diagnostics_json())["work"].clone();
    let before = work(&stepped);
    for _ in 0..10 {
        let _ = stepped.snapshot_json();
        let _ = stepped.overlay_json("viewing");
    }
    assert_eq!(work(&stepped), before);
    assert_eq!(stepped.save_json(), jumped.save_json());
}

const PROOF_PARK: &str = "tests/fixtures/visitor-park.json";

/// The browser proof's park (`apps/web/scripts/visitor-itineraries-proof.mjs`): the three
/// habitats with toilets and stands, staffed, two hours into the day with the first groups in.
/// It must stay a current, loadable save; rewrite it with `ZOO_WRITE_VISITOR_PARK=1`.
#[test]
fn the_visitor_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_VISITOR_PARK").is_some() {
        let mut game = park();
        accepted(game.hire_mechanic());
        accepted(game.hire_janitor());
        game.tick(120);
        let value = json(&game.save_json());
        std::fs::write(
            PROOF_PARK,
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
    let fixture = std::fs::read_to_string(PROOF_PARK).unwrap();
    let mut game = ZooGame::new();
    accepted(game.load_save(&fixture));
    assert_eq!(json(&game.save_json()), json(&fixture));
    let view = json(&game.snapshot_json());
    assert_eq!(view["habitats"].as_array().unwrap().len(), 3);
    assert!(!guests(&view).is_empty());
    game.tick(240);
    assert_eq!(game.session_status(), "running");
}

#[test]
fn itinerary_work_is_one_walk_per_decision() {
    let mut game = park();
    game.tick(720);
    let work = json(&game.diagnostics_json())["work"].clone();
    let decisions = work["itinerary_decisions"].as_u64().unwrap();
    let candidates = work["itinerary_candidates"].as_u64().unwrap();
    let view = json(&game.snapshot_json());
    let groups = view["visits"]["groups_arrived"].as_u64().unwrap();
    assert!(decisions >= groups, "every arrival chose a habitat");
    // Each decision scores at most the park's three habitats.
    assert!(candidates <= decisions * 3, "{work}");
    // Decisions are a small share of the park's route work.
    assert!(work["path_searches"].as_u64().unwrap() >= decisions);
}
