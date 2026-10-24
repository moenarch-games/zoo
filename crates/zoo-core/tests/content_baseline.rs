//! Source-baseline oracle: accepted public command/tick behavior, no new APIs.
use serde_json::{Value, json};
use zoo_core::ZooGame;

fn accepted(result: String) {
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}
pub fn command_fixture(game: &mut ZooGame, species: &str) -> Value {
    accepted(game.place_habitat_rect(3, 8, 7, 10));
    accepted(game.hire_keeper());
    accepted(game.create_work_zone(0, 0, 19, 13));
    accepted(game.assign_keeper_zone(1, 1));
    accepted(game.buy_animal_feed());
    if matches!(species, "elephant" | "giraffe") {
        accepted(game.upgrade_fence(1, "steel_mesh".to_owned()));
    }
    accepted(game.adopt(1, species.to_owned()));
    accepted(game.place_concession(1, 6, "food".to_owned()));
    accepted(game.place_concession(3, 6, "drink".to_owned()));
    game.tick(137);
    let mut snapshot: Value = serde_json::from_str(&game.snapshot_json()).unwrap();
    // #98 adds service-access presentation; the protected behaviour is everything else.
    for stand in snapshot["concessions"].as_array_mut().unwrap() {
        let stand = stand.as_object_mut().unwrap();
        for key in ["customer_tiles", "service_points", "active_service_point"] {
            stand.remove(key);
        }
    }
    json!({
        "cash_cents": snapshot["cash_cents"],
        "habitats": snapshot["habitats"],
        "concessions": snapshot["concessions"],
        "keepers": snapshot["keepers"],
        "guests": snapshot["guests"],
        "finance": snapshot["finance"],
        "operations": snapshot["operations"]
    })
}
#[test]
fn protected_six_species_facility_command_tick_oracle() {
    let rows: Vec<Value> = [
        "capybara", "flamingo", "zebra", "giraffe", "elephant", "penguin",
    ]
    .into_iter()
    .map(
        |species| json!({"species":species,"result":command_fixture(&mut ZooGame::new(), species)}),
    )
    .collect();
    let actual = json!(rows);
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/content-command-baseline.json")).unwrap();
    assert_eq!(actual, expected);
}
