//! #135: work zones, keeper zone assignments and derived habitat membership survive the
//! authoritative save/load path (ordered command log replay) exactly.
use serde_json::Value;
use zoo_core::ZooGame;

fn ok(result: String) {
    let value: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["ok"], true, "{value}");
}

fn rejected(result: String) -> Value {
    let value: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["ok"], false, "{value}");
    value
}

fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}

#[test]
fn replay_reproduces_zone_bounds_keeper_assignments_and_membership() {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.hire_keeper());
    ok(game.hire_keeper());
    ok(game.hire_keeper());
    ok(game.create_work_zone(0, 0, 5, 13));
    ok(game.create_work_zone(6, 0, 12, 13));
    rejected(game.create_work_zone(5, 0, 6, 3));
    ok(game.set_work_zone_bounds(2, 6, 2, 13, 12));
    ok(game.assign_keeper_zone(1, 1));
    ok(game.assign_keeper_zone(2, 2));
    ok(game.schedule_keeper(1));
    ok(game.schedule_keeper(2));
    ok(game.create_work_zone(14, 0, 19, 13));
    ok(game.delete_work_zone(3));
    game.tick(30);

    let view = snapshot(&game);
    let zones = view["work_zones"].as_array().unwrap();
    assert_eq!(zones.len(), 2);
    assert_eq!(
        (
            &zones[1]["x"],
            &zones[1]["y"],
            &zones[1]["width"],
            &zones[1]["height"]
        ),
        (
            &Value::from(6),
            &Value::from(2),
            &Value::from(8),
            &Value::from(11)
        )
    );
    assert_eq!(zones[0]["habitat_ids"], serde_json::json!([1]));
    assert_eq!(zones[1]["keeper_ids"], serde_json::json!([2]));
    assert_eq!(view["habitats"][1]["work_zone_id"], 2);
    let keepers = view["animal_care_depot"]["keepers"].as_array().unwrap();
    assert_eq!(keepers[2]["needs_zone"], true);
    assert_eq!(keepers[2]["status"], "Idle · needs a work zone");

    let content = game.content_json();
    let replayed = ZooGame::replay(&content, &game.diagnostics_json()).expect("log replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}
