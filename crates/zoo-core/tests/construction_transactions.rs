//! #92: construction previews match commits, gestures are all-or-nothing transactions, and
//! bounded undo/redo reverses construction without minting money, consuming IDs or rewinding
//! park time.
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

fn construction_spent(game: &ZooGame) -> i64 {
    snapshot(game)["finance"]["current_day"]["breakdown"]["construction_expense_cents"]
        .as_i64()
        .unwrap()
}

fn tile_kind(game: &ZooGame, x: u32, y: u32) -> String {
    snapshot(game)["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tile| tile["x"] == x && tile["y"] == y)
        .unwrap()["kind"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn history(game: &ZooGame) -> Value {
    snapshot(game)["construction"].clone()
}

fn ids(game: &ZooGame, list: &str) -> Vec<u64> {
    snapshot(game)[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_u64().unwrap())
        .collect()
}

/// The park state that matters for exactness, without the history view.
fn park(game: &ZooGame) -> String {
    game.save_json()
}

#[test]
fn previews_match_commits_under_unchanged_state() {
    let mut game = ZooGame::new();
    let before = cash(&game);
    let preview = json(&game.evaluate_path_stroke(vec![3, 7, 8, 7, 8, 5]));
    assert_eq!(preview["ok"], true, "{preview}");
    // 1..=4 on row 7 already exist; the stroke joins (8,7) to (8,5) without gaps.
    let planned: Vec<(u64, u64)> = preview["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tile| (tile["x"].as_u64().unwrap(), tile["y"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        planned,
        vec![(5, 7), (6, 7), (7, 7), (8, 7), (8, 6), (8, 5)]
    );
    let cost = preview["cost_cents"].as_i64().unwrap();
    assert_eq!(cost, 6 * 1_000);
    ok(game.build_path_stroke(vec![3, 7, 8, 7, 8, 5]));
    assert_eq!(before - cash(&game), cost);
    for (x, y) in planned {
        assert_eq!(tile_kind(&game, x as u32, y as u32), "path");
    }

    let stand = json(&game.evaluate_concession(6, 6, "food".to_owned()));
    assert_eq!(stand["ok"], true, "{stand}");
    let before = cash(&game);
    ok(game.place_concession(6, 6, "food".to_owned()));
    assert_eq!(before - cash(&game), stand["cost_cents"].as_i64().unwrap());

    let fence = json(&game.evaluate_habitat_rect(5, 8, 8, 10));
    assert_eq!(fence["ok"], true, "{fence}");
    let before = cash(&game);
    ok(game.place_habitat_rect(5, 8, 8, 10));
    assert_eq!(before - cash(&game), fence["cost_cents"].as_i64().unwrap());

    let demolition = json(&game.evaluate_demolition(6, 6));
    assert_eq!(demolition["ok"], true);
    assert_eq!(demolition["undoable"], true);
    assert_eq!(demolition["affected"][0]["kind"], "concession");
}

#[test]
fn rejected_actions_leave_the_park_and_cash_exactly_as_they_were() {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(vec![5, 7, 9, 7]));
    ok(game.place_habitat_rect(5, 8, 7, 10));
    let untouched = park(&game);
    let depth = history(&game)["undo_depth"].clone();

    // A stroke that crosses a habitat builds none of its grass tiles.
    let crossing = json(&game.evaluate_path_stroke(vec![4, 9, 9, 9]));
    assert_eq!(crossing["ok"], false);
    assert!(!crossing["blocked_tiles"].as_array().unwrap().is_empty());
    rejected(game.build_path_stroke(vec![4, 9, 9, 9]));
    // A partly out-of-bounds stroke builds nothing either.
    let leaving = rejected(game.build_path_stroke(vec![17, 2, 25, 2]));
    assert!(
        leaving["message"]
            .as_str()
            .unwrap()
            .contains("leaves the park"),
        "{leaving}"
    );
    rejected(game.build_path_stroke(vec![]));
    // Overlapping fence, stand on a path, and demolition of the depot.
    rejected(game.place_habitat_rect(6, 9, 9, 12));
    rejected(game.place_concession(6, 7, "food".to_owned()));
    rejected(game.bulldoze(2, 6));
    assert_eq!(park(&game), untouched);
    assert_eq!(history(&game)["undo_depth"], depth);

    // Insufficient money: a stroke costing one tile more than the cash left is refused whole.
    let mut poor = ZooGame::new();
    while json(&poor.hire_keeper())["ok"] == true {}
    while json(&poor.buy_animal_feed())["ok"] == true {}
    let left = cash(&poor);
    let tiles = u32::try_from(left / 1_000 + 1).unwrap();
    assert!(tiles <= 7, "{left}");
    let before = park(&poor);
    let stroke = vec![5, 7, 4 + tiles, 7];
    let preview = json(&poor.evaluate_path_stroke(stroke.clone()));
    assert_eq!(preview["ok"], false, "{preview}");
    assert!(
        preview["message"]
            .as_str()
            .unwrap()
            .contains("Not enough cash")
    );
    rejected(poor.build_path_stroke(stroke));
    assert_eq!(park(&poor), before);
}

#[test]
fn a_stale_preview_is_not_permission_to_build() {
    let mut game = ZooGame::new();
    let stale = json(&game.evaluate_path_stroke(vec![5, 7, 8, 7]));
    assert_eq!(stale["cost_cents"], 4_000);
    // The park changes after the quote: two of those tiles are now paths.
    ok(game.build_path_stroke(vec![5, 7, 6, 7]));
    let before = cash(&game);
    let built = ok(game.build_path_stroke(vec![5, 7, 8, 7]));
    assert!(
        built["message"].as_str().unwrap().contains("2 new tiles"),
        "{built}"
    );
    assert_eq!(
        before - cash(&game),
        2_000,
        "commit charges the current plan, not the quote"
    );
}

#[test]
fn undo_and_redo_cycles_neither_mint_money_nor_consume_ids() {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(vec![5, 7, 9, 7]));
    ok(game.place_habitat_rect(5, 8, 7, 10));
    ok(game.place_concession(8, 6, "food".to_owned()));
    let built = park(&game);
    let built_cash = cash(&game);
    for _ in 0..3 {
        ok(game.undo_construction());
        ok(game.undo_construction());
        ok(game.undo_construction());
        assert_eq!(
            cash(&game),
            5_000_000,
            "undo refunds exactly what was spent"
        );
        assert_eq!(construction_spent(&game), 0);
        assert_eq!(tile_kind(&game, 6, 7), "grass");
        rejected(game.undo_construction());
        ok(game.redo_construction());
        ok(game.redo_construction());
        ok(game.redo_construction());
        rejected(game.redo_construction());
        assert_eq!(cash(&game), built_cash);
        assert_eq!(
            park(&game),
            built,
            "redo restores the same park and stable IDs"
        );
    }
    assert_eq!(ids(&game, "habitats"), vec![1]);
    assert_eq!(ids(&game, "concessions"), vec![1]);
    ok(game.place_concession(7, 6, "drink".to_owned()));
    assert_eq!(ids(&game, "concessions"), vec![1, 2]);

    // A new transaction clears the redo branch; its reserved ID is simply skipped.
    ok(game.undo_construction());
    ok(game.undo_construction());
    ok(game.place_concession(6, 6, "drink".to_owned()));
    rejected(game.redo_construction());
    assert_eq!(ids(&game, "concessions"), vec![3]);
}

#[test]
fn undo_refuses_used_construction_and_offers_demolition() {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(vec![5, 7, 9, 7]));
    ok(game.place_habitat_rect(5, 8, 7, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.buy_animal_feed());
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.adopt(1, "zebra".to_owned()));
    let before = park(&game);
    let refused = rejected(game.undo_construction());
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("demolish it instead"),
        "{refused}"
    );
    assert_eq!(park(&game), before);
    let view = history(&game);
    assert_eq!(view["undo"]["ok"], false);
    // Demolishing an occupied habitat is irreversible, so it is not undoable either.
    let demolition = json(&game.evaluate_demolition(6, 9));
    assert_eq!(demolition["undoable"], false);
    ok(game.bulldoze(6, 9));
    assert!(ids(&game, "habitats").is_empty());
}

#[test]
fn demolition_of_empty_construction_undoes_and_redoes_free() {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(vec![5, 7, 9, 7]));
    ok(game.place_habitat_rect(5, 8, 7, 10));
    ok(game.place_concession(8, 6, "food".to_owned()));
    let built = park(&game);
    let built_cash = cash(&game);
    ok(game.bulldoze(6, 9));
    ok(game.bulldoze(8, 6));
    ok(game.bulldoze(9, 7));
    assert_eq!(
        cash(&game),
        built_cash,
        "demolition is free and refunds nothing"
    );
    for _ in 0..3 {
        ok(game.undo_construction());
    }
    assert_eq!(park(&game), built);
    for _ in 0..3 {
        ok(game.redo_construction());
    }
    assert!(ids(&game, "habitats").is_empty());
    assert_eq!(cash(&game), built_cash);
}

#[test]
fn undo_never_rewinds_park_time_and_history_ends_with_the_day() {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(vec![5, 7, 9, 7]));
    ok(game.place_habitat_rect(5, 8, 7, 10));
    game.tick(30);
    let clock = (
        snapshot(&game)["day"].clone(),
        snapshot(&game)["minute_of_day"].clone(),
    );
    let before = cash(&game);
    ok(game.undo_construction());
    assert_eq!(
        (
            snapshot(&game)["day"].clone(),
            snapshot(&game)["minute_of_day"].clone()
        ),
        clock
    );
    assert!(cash(&game) > before, "construction is refunded");
    ok(game.redo_construction());
    // Upkeep and wages paid in between are not refunded; the next day closes the history.
    for _ in 0..30 {
        game.tick(60);
    }
    assert_eq!(history(&game)["undo_depth"], 0);
    rejected(game.undo_construction());
}

#[test]
fn history_is_bounded_and_replays_exactly() {
    let mut game = ZooGame::new();
    for x in 5..=19 {
        ok(game.build_path_stroke(vec![x, 7]));
    }
    for y in 8..=12 {
        ok(game.build_path_stroke(vec![19, y]));
    }
    let view = history(&game);
    assert_eq!(view["undo_depth"], view["limit"]);
    let limit = view["limit"].as_u64().unwrap();
    for _ in 0..limit {
        ok(game.undo_construction());
    }
    rejected(game.undo_construction());
    ok(game.redo_construction());
    assert_eq!(tile_kind(&game, 9, 7), "path");
    assert_eq!(tile_kind(&game, 10, 7), "grass");

    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json())
        .expect("undo and redo are recorded commands");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}
