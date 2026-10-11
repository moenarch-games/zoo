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
    accepted(game.schedule_keeper(1));
    accepted(game.buy_animal_feed());
    accepted(game.adopt(1, species.to_owned()));
    accepted(game.place_concession(1, 6, "food".to_owned()));
    accepted(game.place_concession(3, 6, "drink".to_owned()));
    game.tick(137);
    let snapshot: Value = serde_json::from_str(&game.snapshot_json()).unwrap();
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
