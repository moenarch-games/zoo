//! Independent #90 prerequisite acceptance. No scenario/save substitute.
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
#[test]
fn default_catalog_preserves_six_species_public_rules() {
    let expected = json!([
        ["capybara", 25000, 95, 2, 4],
        ["flamingo", 18000, 80, 3, 2],
        ["zebra", 40000, 125, 3, 5],
        ["giraffe", 65000, 160, 2, 8],
        ["elephant", 90000, 190, 2, 12],
        ["penguin", 30000, 110, 4, 3]
    ]);
    let actual: Vec<Value> = view(&ZooGame::new())["species_catalog"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            json!([
                s["key"],
                s["purchase_cost_cents"],
                s["appeal"],
                s["minimum_social_group"],
                s["space_per_animal"]
            ])
        })
        .collect();
    assert_eq!(json!(actual), expected);
}
#[test]
fn bundle_has_versioned_rules_identity_and_is_copied_into_park() {
    let mut definition = catalog();
    assert_eq!(definition["schema_version"], 1);
    assert!(
        definition["ruleset_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    let game = load(&definition);
    let before = game.content_json();
    definition["species"][0]["label"] = json!("Edited source");
    assert_eq!(game.content_json(), before);
    assert_eq!(view(&game)["ruleset_id"], catalog()["ruleset_id"]);
}
#[test]
fn definition_only_species_uses_existing_adoption_and_finance() {
    let mut definition = catalog();
    let mut species = definition["species"][0].clone();
    species["key"] = json!("acceptance_species");
    species["label"] = json!("Acceptance species");
    species["purchase_cost_cents"] = json!(12345);
    definition["species"].as_array_mut().unwrap().push(species);
    let mut game = load(&definition);
    accepted(game.place_habitat_rect(3, 8, 7, 10));
    accepted(game.hire_keeper());
    accepted(game.schedule_keeper(1));
    let before = view(&game)["cash_cents"].as_i64().unwrap();
    accepted(game.adopt(1, "acceptance_species".to_owned()));
    let after = view(&game);
    assert_eq!(after["cash_cents"].as_i64().unwrap(), before - 12345);
    assert_eq!(after["habitats"][0]["species"], "acceptance_species");
    assert_eq!(after["habitats"][0]["animals"], 1);
    assert!(
        after["species_catalog"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["key"] == "acceptance_species")
    );
}
#[test]
fn invalid_catalogs_reject_with_field_paths() {
    let original = catalog();
    let cases = [
        ("schema_version", json!(999)),
        ("species/0/purchase_cost_cents", json!(-1)),
        ("species/0/space_per_animal", json!(0)),
        ("species/0/minimum_social_group", json!(0)),
    ];
    for (path, bad) in cases {
        let mut definition = original.clone();
        *definition.pointer_mut(&format!("/{path}")).unwrap() = bad;
        let error: String = match ZooGame::from_content_json(&definition.to_string()) {
            Ok(_) => panic!("Invalid {path} accepted"),
            Err(error) => error,
        };
        assert!(
            error.contains(path.split('/').next_back().unwrap()),
            "No field path for {path}: {error}"
        );
    }
    let mut duplicate = original.clone();
    let species = duplicate["species"][0].clone();
    duplicate["species"].as_array_mut().unwrap().push(species);
    assert!(ZooGame::from_content_json(&duplicate.to_string()).is_err());
    assert!(ZooGame::from_content_json("{").is_err());
}
#[test]
fn same_rules_and_commands_replay_identically() {
    let definition = catalog();
    let mut first = load(&definition);
    let mut second = load(&definition);
    for game in [&mut first, &mut second] {
        accepted(game.place_habitat_rect(3, 8, 7, 10));
        accepted(game.hire_keeper());
        accepted(game.schedule_keeper(1));
        accepted(game.adopt(1, "capybara".to_owned()));
        game.tick(137);
    }
    assert_eq!(view(&first), view(&second));
}
