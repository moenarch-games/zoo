//! #90: park balance (construction, care, staff, admission and upkeep) comes from the
//! versioned catalog, so editing definitions changes outcomes without code changes.
use serde_json::{Value, json};
use zoo_core::ZooGame;

fn view(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}
fn catalog() -> Value {
    serde_json::from_str(&ZooGame::new().content_json()).unwrap()
}
fn load(definition: &Value) -> ZooGame {
    ZooGame::from_content_json(&definition.to_string()).unwrap()
}
fn accepted(result: String) {
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}
fn cash(game: &ZooGame) -> i64 {
    view(game)["cash_cents"].as_i64().unwrap()
}
fn spent(game: &mut ZooGame, command: impl FnOnce(&mut ZooGame) -> String) -> i64 {
    let before = cash(game);
    accepted(command(game));
    before - cash(game)
}

#[test]
fn edited_balance_drives_construction_staff_and_care_costs() {
    let mut definition = catalog();
    let balance = &mut definition["balance"];
    balance["construction"] = json!({
        "path_tile_cents": 111,
        "habitat_base_cents": 1000,
        "habitat_tile_cents": 10,
        "fence_segment_cents": 1,
    });
    balance["staff"]["keeper"]["hire_cents"] = json!(4321);
    balance["staff"]["janitor"]["hire_cents"] = json!(321);
    balance["habitat_care"]["shelter_cents"] = json!(77);
    balance["feed_batch_cents"] = json!(55);
    let mut game = load(&definition);

    assert_eq!(spent(&mut game, |g| g.place_path(10, 2)), 111);
    // A 5x3 loop: base + 15 tiles + 16 fence segments.
    let preview: Value = serde_json::from_str(&game.evaluate_habitat_rect(3, 8, 7, 10)).unwrap();
    assert_eq!(preview["cost_cents"], 1000 + 15 * 10 + 16);
    assert_eq!(
        spent(&mut game, |g| g.place_habitat_rect(3, 8, 7, 10)),
        1000 + 15 * 10 + 16
    );
    assert_eq!(spent(&mut game, ZooGame::hire_keeper), 4321);
    assert_eq!(spent(&mut game, ZooGame::hire_janitor), 321);
    assert_eq!(spent(&mut game, |g| g.add_shelter(1)), 77);
    assert_eq!(spent(&mut game, ZooGame::buy_animal_feed), 55);
    let depot = &view(&game)["animal_care_depot"];
    assert_eq!(depot["keeper_hire_cost_cents"], 4321);
    assert_eq!(depot["feed_batch_cost_cents"], 55);
}

fn hourly_park_upkeep(definition: &Value) -> i64 {
    let mut game = load(definition);
    accepted(game.place_habitat_rect(3, 8, 7, 10));
    accepted(game.hire_keeper());
    accepted(game.create_work_zone(0, 0, 19, 13));
    accepted(game.assign_keeper_zone(1, 1));
    accepted(game.adopt(1, "capybara".to_owned()));
    game.tick(60);
    view(&game)["finance"]["current_day"]["breakdown"]["park_upkeep_expense_cents"]
        .as_i64()
        .unwrap()
}

#[test]
fn species_and_park_upkeep_come_from_definitions() {
    let original = catalog();
    let baseline = hourly_park_upkeep(&original);
    let mut pricier = original.clone();
    let capybara = pricier["species"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|species| species["key"] == "capybara")
        .unwrap();
    let per_animal = capybara["hourly_upkeep_cents"].as_i64().unwrap();
    capybara["hourly_upkeep_cents"] = json!(per_animal + 1000);
    pricier["balance"]["upkeep"]["habitat_hourly_cents"] = json!(
        original["balance"]["upkeep"]["habitat_hourly_cents"]
            .as_i64()
            .unwrap()
            + 7
    );
    // One adopted animal and one habitat.
    assert_eq!(hourly_park_upkeep(&pricier), baseline + 1000 + 7);
}

#[test]
fn invalid_balance_values_name_their_field_path() {
    for (pointer, bad, path) in [
        (
            "/balance/staff/mechanic/hourly_wage_cents",
            json!(-1),
            "balance/staff/mechanic/hourly_wage_cents",
        ),
        (
            "/balance/construction/path_tile_cents",
            json!(i64::MAX),
            "balance/construction/path_tile_cents",
        ),
        (
            "/species/0/hourly_upkeep_cents",
            json!(-5),
            "species/0/hourly_upkeep_cents",
        ),
        (
            "/facilities/0/hourly_upkeep_cents",
            json!(-5),
            "facilities/0/hourly_upkeep_cents",
        ),
    ] {
        let mut definition = catalog();
        *definition.pointer_mut(pointer).unwrap() = bad;
        let error = ZooGame::from_content_json(&definition.to_string())
            .err()
            .unwrap_or_else(|| panic!("accepted invalid {pointer}"));
        assert!(error.contains(path), "{pointer}: {error}");
    }
    let mut missing = catalog();
    missing["balance"].as_object_mut().unwrap().remove("upkeep");
    let error = ZooGame::from_content_json(&missing.to_string())
        .err()
        .unwrap();
    assert!(
        error.contains("balance") && error.contains("upkeep"),
        "{error}"
    );
    let mut fixed = catalog();
    let entrance = fixed["facilities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|facility| facility["key"] == "entrance")
        .unwrap();
    entrance["hourly_upkeep_cents"] = json!(10);
    let error = ZooGame::from_content_json(&fixed.to_string())
        .err()
        .unwrap();
    assert!(error.contains("hourly_upkeep_cents"), "{error}");
}

#[test]
fn a_previous_content_version_reports_the_version_not_a_missing_field() {
    let mut old = catalog();
    old["schema_version"] = json!(1);
    old.as_object_mut().unwrap().remove("balance");
    let error = ZooGame::from_content_json(&old.to_string()).err().unwrap();
    assert!(
        error.contains("schema_version") && !error.contains("balance"),
        "{error}"
    );
}
