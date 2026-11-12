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
        // #101 adds guest-comfort presentation; stands have none.
        assert_eq!(stand.remove("comfort"), Some(Value::Null));
    }
    // #101 adds each guest's toilet need and errand, and comfort operations; with no toilets
    // or benches built, they change nothing protected here.
    for guest in snapshot["guests"].as_array_mut().unwrap() {
        guest.as_object_mut().unwrap().remove("comfort").unwrap();
    }
    snapshot["operations"]
        .as_object_mut()
        .unwrap()
        .remove("comfort")
        .unwrap();
    // #108 lists a habitat's resident species, each with its own requirements; with one
    // species per habitat that is exactly the species and requirements it had.
    for habitat in snapshot["habitats"].as_array_mut().unwrap() {
        let habitat = habitat.as_object_mut().unwrap();
        let species = habitat["species"].as_array().unwrap().clone();
        assert!(species.len() <= 1);
        let residents = habitat.remove("residents").unwrap();
        assert_eq!(residents.as_array().unwrap().len(), species.len());
        habitat.insert(
            "species".to_owned(),
            species.first().cloned().unwrap_or(Value::Null),
        );
        habitat.insert(
            "requirements".to_owned(),
            residents[0]["requirements"].clone(),
        );
    }
    // #99 adds a land-purchase ledger line, always zero here.
    for day in ["current_day", "previous_day"] {
        if let Some(breakdown) = snapshot["finance"]
            .get_mut(day)
            .and_then(|day| day.get_mut("breakdown"))
            .and_then(Value::as_object_mut)
        {
            assert_eq!(breakdown.remove("land_expense_cents"), Some(json!(0)));
            // #109 adds veterinary ledger lines, always zero here.
            for key in [
                "vet_hiring_expense_cents",
                "vet_wages_expense_cents",
                "medicine_expense_cents",
            ] {
                assert_eq!(breakdown.remove(key), Some(json!(0)));
            }
        }
    }
    // #109 marks quarantine habitats and treatment patients; there are none here.
    for habitat in snapshot["habitats"].as_array_mut().unwrap() {
        let habitat = habitat.as_object_mut().unwrap();
        assert_eq!(habitat.remove("quarantine"), Some(json!(false)));
        for job in habitat["care_jobs"].as_array_mut().unwrap() {
            assert_eq!(
                job.as_object_mut().unwrap().remove("animal_id"),
                Some(Value::Null)
            );
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
    // #102 deliberately changes stand behaviour (queues, service time, vendor wages, stock), so
    // the oracle was re-recorded with ZOO_WRITE_CONTENT_BASELINE=1 at that change. #103 did the
    // same: guests arrive in groups and choose habitats by profile. #104 did too: keepers
    // collect feed at the depot and walk to care spots inside habitats. #106 did too: guests pay
    // admission from one seeded budget and arrive valuing the visit by its price.
    if std::env::var_os("ZOO_WRITE_CONTENT_BASELINE").is_some() {
        std::fs::write(
            "tests/fixtures/content-command-baseline.json",
            serde_json::to_string(&actual).unwrap(),
        )
        .unwrap();
    }
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/content-command-baseline.json")).unwrap();
    assert_eq!(actual, expected);
}
