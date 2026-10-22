use serde_json::Value;
use zoo_core::ZooGame;

fn accepted(result: String) {
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}

fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

fn habitat(view: &Value, id: u32) -> &Value {
    view["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|habitat| habitat["id"] == id)
        .unwrap()
}

#[test]
fn path_frontage_adds_capacity_only_where_animals_are_visible() {
    let mut game = ZooGame::new();
    accepted(game.place_habitat_rect(3, 8, 7, 10));
    let habitat_id = snapshot(&game)["habitats"][0]["id"].as_u64().unwrap() as u32;
    accepted(game.hire_keeper());
    accepted(game.create_work_zone(0, 0, 19, 13));
    accepted(game.assign_keeper_zone(1, 1));
    accepted(game.schedule_keeper(habitat_id));
    accepted(game.adopt(habitat_id, "capybara".to_owned()));

    let before = snapshot(&game);
    let habitat_before = habitat(&before, habitat_id);
    assert_eq!(habitat_before["viewing_capacity"], 2);

    let spots = habitat_before["viewing_spots"].as_array().unwrap();
    assert_eq!(spots.len(), 2);
    assert_eq!(
        spots
            .iter()
            .filter(|spot| spot["visible_animals"].as_u64().unwrap() > 0)
            .count(),
        1
    );

    accepted(game.place_path(5, 7));
    let after_path = snapshot(&game);
    assert_eq!(habitat(&after_path, habitat_id)["viewing_capacity"], 4);

    accepted(game.adopt(habitat_id, "capybara".to_owned()));
    let after_second_animal = snapshot(&game);
    assert_eq!(
        habitat(&after_second_animal, habitat_id)["viewing_capacity"],
        8
    );
}
