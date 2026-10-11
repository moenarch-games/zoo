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

#[path = "content_baseline.rs"]
mod baseline;

#[test]
fn loaded_default_catalog_matches_protected_command_tick_fixtures() {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/content-command-baseline.json")).unwrap();
    for row in oracle.as_array().unwrap() {
        let mut game = load(&catalog());
        let actual = baseline::command_fixture(&mut game, row["species"].as_str().unwrap());
        assert_eq!(actual, row["result"], "{}", row["species"]);
    }
}

#[test]
fn catalog_facilities_preserve_accepted_costs_and_one_tile_footprints() {
    let definition = catalog();
    for (key, build_cost, price) in [("food", 18000, 500), ("drink", 14000, 350)] {
        let facility = definition["facilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == key)
            .unwrap();
        assert_eq!(facility["build_cost_cents"], build_cost);
        assert_eq!(facility["price_cents"], price);
        assert_eq!(facility["footprint"], json!({"width":1,"height":1}));
    }
}

#[test]
fn definition_only_facility_uses_real_placement_cost_and_stable_content_id() {
    let mut definition = catalog();
    let mut facility = definition["facilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "food")
        .unwrap()
        .clone();
    facility["key"] = json!("acceptance_food_facility");
    facility["label"] = json!("Acceptance food facility");
    facility["build_cost_cents"] = json!(12345);
    facility["price_cents"] = json!(456);
    definition["facilities"]
        .as_array_mut()
        .unwrap()
        .push(facility);
    let mut game = load(&definition);
    let before = view(&game)["cash_cents"].as_i64().unwrap();
    accepted(game.place_concession(1, 6, "acceptance_food_facility".into()));
    let after = view(&game);
    assert_eq!(after["cash_cents"].as_i64().unwrap(), before - 12345);
    assert_eq!(after["concessions"][0]["kind"], "acceptance_food_facility");
    assert_eq!(after["concessions"][0]["price_cents"], 456);
    // Repeated placement remains idempotent through the existing command seam.
    accepted(game.place_concession(1, 6, "acceptance_food_facility".into()));
    assert_eq!(view(&game), after);
}

fn rejected_field(definition: &Value, field: &str) {
    let error = match ZooGame::from_content_json(&definition.to_string()) {
        Ok(_) => panic!("invalid {field} accepted"),
        Err(error) => error,
    };
    assert!(
        error.contains(field),
        "missing useful {field} path: {error}"
    );
}

#[test]
fn asset_unlock_footprint_cost_and_payload_bounds_fail_closed() {
    let original = catalog();
    for (path, value, field) in [
        ("/facilities/0/footprint/width", json!(0), "width"),
        ("/facilities/0/footprint/height", json!(u64::MAX), "height"),
        (
            "/facilities/0/build_cost_cents",
            json!(-1),
            "build_cost_cents",
        ),
        (
            "/facilities/0/build_cost_cents",
            json!(u64::MAX),
            "build_cost_cents",
        ),
        ("/facilities/0/price_cents", json!(i64::MAX), "price_cents"),
        (
            "/species/0/purchase_cost_cents",
            json!(i64::MAX),
            "purchase_cost_cents",
        ),
        (
            "/species/0/asset_ref",
            json!("missing-real-asset"),
            "asset_ref",
        ),
        (
            "/facilities/0/asset_ref",
            json!("missing-real-asset"),
            "asset_ref",
        ),
        (
            "/species/0/unlock_ref",
            json!("missing-unlock"),
            "unlock_ref",
        ),
        (
            "/facilities/0/unlock_ref",
            json!("missing-unlock"),
            "unlock_ref",
        ),
    ] {
        let mut changed = original.clone();
        let components: Vec<_> = path.trim_start_matches('/').split('/').collect();
        let leaf = components.last().unwrap();
        let parent = format!("/{}", components[..components.len() - 1].join("/"));
        changed
            .pointer_mut(&parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert((*leaf).to_owned(), value);
        rejected_field(&changed, field);
    }
    let mut duplicate = original.clone();
    let facility = duplicate["facilities"][0].clone();
    duplicate["facilities"]
        .as_array_mut()
        .unwrap()
        .push(facility);
    rejected_field(&duplicate, "facilities");
    let mut oversized = original;
    // Multi-megabyte payload must fail before creating a park, not become unbounded content.
    oversized["species"][0]["label"] = json!("x".repeat(16 * 1024 * 1024));
    assert!(ZooGame::from_content_json(&oversized.to_string()).is_err());
}

#[test]
fn localization_and_valid_presentation_changes_do_not_change_simulation_decisions() {
    let original = catalog();
    let mut cosmetic = original.clone();
    for species in cosmetic["species"].as_array_mut().unwrap() {
        species["label"] = json!("Localized animal");
    }
    // An already valid real presentation reference is cosmetic, not a rules change.
    let alternate_asset = original["species"][1]["asset_ref"].clone();
    assert!(alternate_asset.as_str().is_some_and(|id| !id.is_empty()));
    cosmetic["species"][0]["asset_ref"] = alternate_asset;
    for facility in cosmetic["facilities"].as_array_mut().unwrap() {
        facility["label"] = json!("Localized stand");
    }
    // Keep actual verified assets; this contract never supplies proxy/generated art.
    for name in [
        "capybara", "flamingo", "zebra", "giraffe", "elephant", "penguin",
    ] {
        let mut first = load(&original);
        let mut second = load(&cosmetic);
        let mut a = baseline::command_fixture(&mut first, name);
        let mut b = baseline::command_fixture(&mut second, name);
        // Presentation messages may use labels. Retain every numeric/state/ID decision.
        fn remove_messages(value: &mut Value) {
            match value {
                Value::Object(fields) => {
                    for key in ["label", "feeding_status", "maintenance_status", "status"] {
                        fields.remove(key);
                    }
                    for child in fields.values_mut() {
                        remove_messages(child);
                    }
                }
                Value::Array(values) => {
                    for child in values {
                        remove_messages(child);
                    }
                }
                _ => {}
            }
        }
        remove_messages(&mut a);
        remove_messages(&mut b);
        assert_eq!(a, b, "cosmetic edit changed decisions for {name}");
    }
}
