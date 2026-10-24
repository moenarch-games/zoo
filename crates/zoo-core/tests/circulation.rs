//! Park circulation through the public API (#98): route-work counters for representative edits,
//! and replay of widened strokes, plazas and access changes.
use serde_json::Value;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
}

fn work(game: &ZooGame) -> Value {
    json(&game.diagnostics_json())["work"].clone()
}

fn count(work: &Value, name: &str) -> u64 {
    work[name]
        .as_u64()
        .unwrap_or_else(|| panic!("{name} in {work}"))
}

type Edit = fn(&mut ZooGame) -> String;

/// A stocked habitat with guests walking a public loop past a separate staff-only route.
fn busy_park() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.build_path_stroke(
        vec![5, 7, 12, 7, 12, 12, 5, 12, 5, 8],
        2,
        "public".to_owned(),
    ));
    ok(game.place_habitat_rect(7, 9, 11, 11));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.build_path_stroke(vec![3, 6, 3, 2, 15, 2, 15, 6], 1, "staff".to_owned()));
    for _ in 0..90 {
        game.tick(1);
    }
    game
}

/// Correctness is covered by the circulation unit tests; this records how much route work each
/// representative edit costs and checks only that every path edit advances the topology
/// revision once, and that an edit away from every route invalidates none.
#[test]
fn route_work_counters_are_recorded_for_representative_edits() {
    let mut game = busy_park();
    assert!(
        json(&game.snapshot_json())["guests"]
            .as_array()
            .is_some_and(|guests| !guests.is_empty()),
        "guests are walking"
    );
    let edits: [(&str, Edit); 5] = [
        ("unrelated staff lane", |game| {
            game.build_path_stroke(vec![17, 0, 19, 0], 1, "staff".to_owned())
        }),
        ("widen a bottleneck", |game| {
            game.build_path_stroke(vec![1, 8, 4, 8], 1, "public".to_owned())
        }),
        ("public plaza", |game| {
            game.build_plaza(13, 9, 14, 11, "public".to_owned())
        }),
        ("staff-only reclassification", |game| {
            game.build_path_stroke(vec![6, 7, 6, 8], 1, "staff".to_owned())
        }),
        ("demolish a tile", |game| game.bulldoze(15, 4)),
    ];
    for (index, (label, edit)) in edits.into_iter().enumerate() {
        let before = work(&game);
        let result = json(&edit(&mut game));
        let after = work(&game);
        let searches = count(&after, "path_searches") - count(&before, "path_searches");
        let invalidated =
            count(&after, "route_invalidations") - count(&before, "route_invalidations");
        eprintln!(
            "{label}: ok={} path_searches={searches} route_invalidations={invalidated}",
            result["ok"]
        );
        if result["ok"] == true {
            assert_eq!(
                count(&after, "topology_revision"),
                count(&before, "topology_revision") + 1,
                "{label}"
            );
        }
        if index == 0 {
            assert_eq!(invalidated, 0, "an unrelated edit re-plans nothing");
        }
    }
}

#[test]
fn widened_strokes_plazas_and_access_changes_replay_exactly() {
    let mut game = busy_park();
    ok(game.build_plaza(13, 9, 14, 11, "public".to_owned()));
    ok(game.build_path_stroke(vec![12, 9, 13, 9], 1, "staff".to_owned()));
    ok(game.undo_construction());
    ok(game.redo_construction());
    game.tick(60);
    let content = ZooGame::new().content_json();
    let replayed = ZooGame::replay(&content, &game.diagnostics_json()).expect("replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}
