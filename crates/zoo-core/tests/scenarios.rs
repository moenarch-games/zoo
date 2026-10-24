//! #91: scenarios start distinct, reproducible parks that play, save and load like any other.
use serde_json::Value;
use zoo_core::{ZooGame, scenario_catalog_json};

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) -> Value {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn scenario_ids() -> Vec<String> {
    json(&scenario_catalog_json())
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap().to_owned())
        .collect()
}

fn started(id: &str) -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.start_scenario(id));
    game
}

/// The same opening commands on every map: extend the entrance path and play two hours.
fn play_opening(game: &mut ZooGame) {
    let view = json(&game.snapshot_json());
    let entrance = (
        view["entrance"]["x"].as_u64().unwrap() as u32,
        view["entrance"]["y"].as_u64().unwrap() as u32,
    );
    // Every built-in entrance is on the west edge; build one more path tile eastwards.
    let (x, y) = (entrance.0 + 1, entrance.1);
    ok(game.place_path(x, y));
    for _ in 0..8 {
        game.tick(15);
    }
}

#[test]
fn the_picker_offers_the_agreed_starting_conditions_in_order() {
    let catalog = json(&scenario_catalog_json());
    let ids = scenario_ids();
    assert_eq!(
        ids,
        [
            "empty-starter",
            "partly-prepared",
            "living-starter",
            "recovery"
        ]
    );
    assert_eq!(
        catalog[0]["keeper_training"], true,
        "keeper training lives in the empty starter"
    );
    for entry in catalog.as_array().unwrap() {
        assert!(
            entry["title"]
                .as_str()
                .is_some_and(|title| !title.is_empty())
        );
        assert!(entry["width"].as_u64().unwrap() >= 8);
    }
}

#[test]
fn every_scenario_starts_plays_saves_and_loads() {
    for id in scenario_ids() {
        let mut game = started(&id);
        let view = json(&game.snapshot_json());
        assert_eq!(view["scenario"]["id"], id.as_str());
        assert_eq!(
            view["tiles"].as_array().unwrap().len() as u64,
            view["width"].as_u64().unwrap() * view["height"].as_u64().unwrap()
        );
        let today = &view["finance"]["current_day"];
        assert_eq!(
            today["expenses_cents"], 0,
            "{id}: starting objects are free"
        );
        assert_eq!(today["income_cents"], 0, "{id}: no phantom income");

        play_opening(&mut game);
        let saved = game.save_json();
        assert_eq!(json(&saved)["park"]["scenario"]["id"], id.as_str());
        let mut reloaded = ZooGame::new();
        ok(reloaded.load_save(&saved));
        assert_eq!(reloaded.save_json(), saved, "{id}: save/load round trip");
        for park in [&mut game, &mut reloaded] {
            for _ in 0..4 {
                park.tick(15);
            }
        }
        assert_eq!(
            reloaded.save_json(),
            game.save_json(),
            "{id}: reload continues identically"
        );
    }
}

#[test]
fn identical_scenario_and_commands_reproduce_identical_parks() {
    for id in scenario_ids() {
        let mut first = started(&id);
        let mut second = started(&id);
        assert_eq!(
            first.save_json(),
            second.save_json(),
            "{id}: identical start"
        );
        play_opening(&mut first);
        play_opening(&mut second);
        assert_eq!(
            first.save_json(),
            second.save_json(),
            "{id}: identical results"
        );
    }
}

#[test]
fn a_scenario_session_replays_from_its_recorded_start() {
    let mut game = started("living-starter");
    play_opening(&mut game);
    let bundle = json(&game.diagnostics_json());
    assert_eq!(bundle["start"]["scenario"], "living-starter");
    assert!(bundle["start"]["scenario_digest"].is_string());
    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json()).unwrap();
    assert_eq!(replayed.save_json(), game.save_json());
}

#[test]
fn the_living_starter_operates_without_player_input() {
    let mut game = started("living-starter");
    for _ in 0..16 {
        game.tick(15);
    }
    let view = json(&game.snapshot_json());
    assert!(
        !view["guests"].as_array().unwrap().is_empty(),
        "guests arrive"
    );
    assert!(
        !view["animals"].as_array().unwrap().is_empty(),
        "animals live there"
    );
    assert!(
        view["finance"]["current_day"]["income_cents"]
            .as_i64()
            .unwrap()
            > 0
    );
}

#[test]
fn a_new_scenario_starts_with_an_empty_undo_history() {
    for id in scenario_ids() {
        let mut game = started("empty-starter");
        ok(game.place_path(5, 7));
        ok(game.start_scenario(&id));
        let view = json(&game.snapshot_json());
        assert_eq!(
            view["construction"]["undo_depth"], 0,
            "{id}: authored setup or the previous park left undoable construction: {}",
            view["construction"]
        );
        let before = game.save_json();
        assert_eq!(json(&game.undo_construction())["ok"], false);
        assert_eq!(
            game.save_json(),
            before,
            "{id}: undo changed the starting park"
        );
    }
}

#[test]
fn an_unknown_scenario_leaves_the_current_park_untouched() {
    let mut game = started("recovery");
    let before = game.save_json();
    let result = json(&game.start_scenario("no-such-park"));
    assert_eq!(result["ok"], false);
    assert_eq!(game.save_json(), before);
}

#[test]
fn objectives_latch_from_park_state() {
    let mut game = started("recovery");
    let view = json(&game.snapshot_json());
    let objectives = view["scenario"]["objectives"].as_array().unwrap();
    assert_eq!(objectives[0]["id"], "restock-feed");
    assert_eq!(objectives[0]["done"], false);
    assert_eq!(
        view["scenario"]["opening_focus"],
        serde_json::json!({ "x": 6, "y": 4 })
    );
    ok(game.buy_animal_feed());
    let view = json(&game.snapshot_json());
    assert_eq!(view["scenario"]["objectives"][0]["done"], true);
}

#[test]
fn locked_content_is_not_offered_or_adoptable() {
    let mut game = started("recovery");
    let view = json(&game.snapshot_json());
    let offered: Vec<&str> = view["species_catalog"]
        .as_array()
        .unwrap()
        .iter()
        .map(|species| species["key"].as_str().unwrap())
        .collect();
    assert_eq!(offered, ["capybara", "flamingo"]);
    let result = json(&game.adopt(1, "zebra".to_owned()));
    assert_eq!(result["ok"], false);
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("not available")
    );
}
