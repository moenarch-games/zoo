//! Retrospective independent expectations for #140/#142, before the next repair.
//! This manual park is technical core evidence, never approved art/visual proof.
use serde_json::Value;
use zoo_core::ZooGame;

fn accepted(result: String) {
    assert_eq!(serde_json::from_str::<Value>(&result).unwrap()["ok"], true);
}

fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

fn populated() -> ZooGame {
    let mut game = ZooGame::new();
    accepted(game.place_habitat_rect(5, 5, 8, 7));
    accepted(game.place_path(4, 6));
    accepted(game.place_path(4, 5));
    accepted(game.hire_keeper());
    accepted(game.schedule_keeper(1));
    accepted(game.buy_animal_feed());
    accepted(game.add_shelter(1));
    accepted(game.adopt(1, "capybara".into()));
    accepted(game.adopt(1, "capybara".into()));
    game
}

#[test]
fn characterization_real_populated_park_has_distinct_animals_and_valid_guest_viewing() {
    let mut game = populated();
    let animals = snapshot(&game)["animals"].as_array().unwrap().clone();
    assert_eq!(animals.len(), 2);
    assert_ne!(animals[0]["id"], animals[1]["id"]);
    let mut viewed = false;
    for _ in 0..180 {
        game.tick(1);
        let state = snapshot(&game);
        viewed |= state["guests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guest| guest["state"] == "viewing" && guest["target_habitat"] == 1);
    }
    assert!(viewed);
}

#[test]
fn individual_name_and_identity_survive_ticks_and_another_real_adoption() {
    let mut game = populated();
    let animals = snapshot(&game)["animals"].as_array().unwrap().clone();
    for animal in &animals {
        assert!(
            animal["name"]
                .as_str()
                .is_some_and(|name| !name.trim().is_empty()),
            "individual name must originate in zoo-core: {animal}"
        );
    }
    accepted(game.adopt(1, "capybara".into()));
    game.tick(17);
    let after = snapshot(&game);
    let after = after["animals"].as_array().unwrap();
    for original in animals {
        let retained = after
            .iter()
            .find(|animal| animal["id"] == original["id"])
            .unwrap();
        assert_eq!(retained["name"], original["name"]);
        assert_eq!(retained["habitat_id"], original["habitat_id"]);
    }
}

#[test]
fn purposeful_walking_feeding_and_resting_are_actual_deterministic_core_state() {
    let mut first = populated();
    let mut second = populated();
    let mut activities = std::collections::BTreeSet::new();
    let mut previous = std::collections::BTreeMap::<u64, (String, Value)>::new();
    let mut transitioned = false;
    for _ in 0..180 {
        let state = snapshot(&first);
        assert_eq!(first.snapshot_json(), second.snapshot_json());
        for animal in state["animals"].as_array().unwrap() {
            let activity = animal["activity"]
                .as_str()
                .expect("activity is core state, not renderer formula motion")
                .to_lowercase();
            activities.insert(activity.clone());
            if matches!(activity.as_str(), "walking" | "feeding" | "resting") {
                assert!(
                    !animal["activity_target"].is_null(),
                    "purposeful activity needs its authoritative target"
                );
            }
            let id = animal["id"]
                .as_u64()
                .expect("stable actual core individual ID");
            if let Some((prior, target)) = previous.get(&id)
                && ((prior == "walking" && matches!(activity.as_str(), "feeding" | "resting"))
                    || (activity == "walking" && matches!(prior.as_str(), "feeding" | "resting")))
            {
                assert!(!target.is_null() && !animal["activity_target"].is_null());
                transitioned = true;
            }
            previous.insert(id, (activity, animal["activity_target"].clone()));
        }
        first.tick(1);
        second.tick(1);
    }
    assert!(
        ["walking", "feeding", "resting"]
            .iter()
            .all(|activity| activities.contains(*activity)),
        "real finite fixture must supply all selected living activities: {activities:?}"
    );
    assert!(
        transitioned,
        "same stable individual must actually transition Walking↔Feeding/Resting across real ticks; static activity assignments are insufficient"
    );
}
