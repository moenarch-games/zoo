//! #95: content-defined fence types, species minimum fence heights and whole-fence upgrades.
use serde_json::Value;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) -> Value {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn rejected(result: String) -> Value {
    let value = json(&result);
    assert_eq!(value["ok"], false, "{value}");
    value
}

fn snapshot(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

fn cash(game: &ZooGame) -> i64 {
    snapshot(game)["cash_cents"].as_i64().unwrap()
}

fn fence(game: &ZooGame) -> Value {
    snapshot(game)["habitats"][0]["fence"].clone()
}

const STEEL: &str = "steel_mesh";
/// Built-in steel mesh price per segment.
const STEEL_SEGMENT: i64 = 3_000;

/// A staffed 4×3 rectangle (fence 14) under the starter path.
fn staffed() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.place_habitat_rect(3, 8, 6, 10));
    ok(game.hire_keeper());
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    game
}

#[test]
fn the_catalog_defines_a_low_wooden_rail_and_a_tall_steel_mesh() {
    let game = ZooGame::new();
    let catalog = snapshot(&game)["fence_catalog"].clone();
    let keys: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["wooden_rail", STEEL]);
    assert!(catalog[1]["height_cm"].as_u64() > catalog[0]["height_cm"].as_u64());
    let species = snapshot(&game)["species_catalog"].clone();
    let giraffe = species
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "giraffe")
        .unwrap();
    assert!(giraffe["minimum_fence_height_cm"].as_u64() > catalog[0]["height_cm"].as_u64());
}

#[test]
fn a_low_fence_blocks_tall_species_until_the_fence_is_upgraded() {
    let mut game = staffed();
    assert_eq!(fence(&game)["key"], "wooden_rail");
    // Capybaras are fine behind the wooden rail; giraffes are not.
    let before = cash(&game);
    let refusal = rejected(game.adopt(1, "giraffe".to_owned()));
    assert!(
        refusal["message"]
            .as_str()
            .unwrap()
            .contains("Upgrade its fence"),
        "{refusal}"
    );
    assert_eq!(cash(&game), before);

    let preview = json(&game.evaluate_fence_upgrade(1, STEEL.to_owned()));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["action"], "fence");
    assert_eq!(preview["cost_cents"], 14 * STEEL_SEGMENT);
    ok(game.upgrade_fence(1, STEEL.to_owned()));
    assert_eq!(
        before - cash(&game),
        14 * STEEL_SEGMENT,
        "full price, nothing refunded"
    );
    assert_eq!(fence(&game)["key"], STEEL);
    // Upgrading to the same type again is a free no-op.
    let upgraded = cash(&game);
    ok(game.upgrade_fence(1, STEEL.to_owned()));
    assert_eq!(cash(&game), upgraded);

    ok(game.adopt(1, "giraffe".to_owned()));
    // The animals now need the tall fence: downgrades and undo both refuse.
    let downgrade = json(&game.evaluate_fence_upgrade(1, "wooden_rail".to_owned()));
    assert_eq!(downgrade["ok"], false);
    rejected(game.upgrade_fence(1, "wooden_rail".to_owned()));
    // The adoption is not construction, so undo targets the fence change and refuses it.
    let undo = rejected(game.undo_construction());
    assert!(
        undo["message"].as_str().unwrap().contains("needs a fence"),
        "{undo}"
    );
    assert_eq!(fence(&game)["key"], STEEL);
    rejected(game.upgrade_fence(1, "barbed_wire".to_owned()));
}

#[test]
fn fence_changes_undo_and_redo_at_the_exact_price() {
    let mut game = staffed();
    let before = cash(&game);
    ok(game.upgrade_fence(1, STEEL.to_owned()));
    ok(game.undo_construction());
    assert_eq!(cash(&game), before);
    assert_eq!(fence(&game)["key"], "wooden_rail");
    ok(game.redo_construction());
    assert_eq!(before - cash(&game), 14 * STEEL_SEGMENT);
    assert_eq!(fence(&game)["key"], STEEL);
}

#[test]
fn reshaping_prices_new_segments_and_upkeep_by_fence_type() {
    let mut wooden = staffed();
    let mut steel = staffed();
    ok(steel.upgrade_fence(1, STEEL.to_owned()));
    // Grow the rectangle by a 2×2 leg: 4 tiles and 6 new fence segments.
    let grown = vec![3, 8, 7, 8, 7, 11, 5, 11, 5, 13, 3, 13, 3, 8];
    let wooden_cost = json(&wooden.evaluate_habitat_reshape(1, grown.clone()))["cost_cents"]
        .as_i64()
        .unwrap();
    let steel_cost = json(&steel.evaluate_habitat_reshape(1, grown))["cost_cents"]
        .as_i64()
        .unwrap();
    assert_eq!(steel_cost - wooden_cost, 6 * (STEEL_SEGMENT - 1_500));

    let upkeep = |game: &mut ZooGame| {
        let before =
            snapshot(game)["finance"]["current_day"]["breakdown"]["park_upkeep_expense_cents"]
                .as_i64()
                .unwrap();
        game.tick(60);
        snapshot(game)["finance"]["current_day"]["breakdown"]["park_upkeep_expense_cents"]
            .as_i64()
            .unwrap()
            - before
    };
    assert_eq!(upkeep(&mut steel) - upkeep(&mut wooden), 14 * (12 - 8));
}

#[test]
fn fence_upgrades_replay_from_the_session_log() {
    let mut game = staffed();
    ok(game.upgrade_fence(1, STEEL.to_owned()));
    ok(game.adopt(1, "giraffe".to_owned()));
    game.tick(30);
    let content = ZooGame::new().content_json();
    let replayed = ZooGame::replay(&content, &game.diagnostics_json()).expect("replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}
