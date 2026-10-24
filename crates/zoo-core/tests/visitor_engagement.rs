use serde_json::Value;
use zoo_core::ZooGame;

fn accepted(result: String) {
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}

fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

/// Covers the park with one work zone (once) and assigns the newest keeper to it.
fn zone_newest_keeper(game: &mut ZooGame) {
    let view = snapshot(game);
    if view["work_zones"].as_array().unwrap().is_empty() {
        accepted(game.create_work_zone(0, 0, 19, 13));
    }
    let keeper = view["animal_care_depot"]["keepers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    accepted(game.assign_keeper_zone(keeper, 1));
}

fn populate(game: &mut ZooGame, rect: (u32, u32, u32, u32), species: &str) -> u32 {
    accepted(game.place_habitat_rect(rect.0, rect.1, rect.2, rect.3));
    let view = snapshot(game);
    let habitat_id = view["habitats"].as_array().unwrap().last().unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    accepted(game.hire_keeper());
    zone_newest_keeper(game);
    if matches!(species, "elephant" | "giraffe") {
        accepted(game.upgrade_fence(habitat_id, "steel_mesh".to_owned()));
    }
    accepted(game.adopt(habitat_id, species.to_owned()));
    habitat_id
}

fn engagement_park() -> ZooGame {
    let mut game = ZooGame::new();
    populate(&mut game, (5, 5, 8, 7), "capybara");
    accepted(game.place_path(10, 7));
    populate(&mut game, (11, 5, 14, 7), "giraffe");
    for x in 4..=10 {
        accepted(game.place_path(x, 8));
    }
    game
}

#[test]
fn visitor_engagement_progression_is_observable_and_deterministic() {
    let mut first = engagement_park();
    let mut second = engagement_park();
    let mut observed_multiple_habitats = false;

    for _ in 0..180 {
        first.tick(1);
        second.tick(1);
        assert_eq!(first.snapshot_json(), second.snapshot_json());

        observed_multiple_habitats |= snapshot(&first)["guests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guest| guest["id"] == 1 && guest["habitats_viewed"].as_u64().unwrap_or(0) >= 2);
    }

    assert!(
        observed_multiple_habitats,
        "guest #1 never exposed multi-habitat engagement in the public snapshot"
    );
}
