//! #96: animals are persistent individuals with stable IDs, names, positions inside their
//! habitat, an authoritative activity and needs, through saves, replays and any frame rate.
use serde_json::Value;
use std::collections::HashSet;
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

fn animals(game: &ZooGame) -> Vec<Value> {
    json(&game.snapshot_json())["animals"]
        .as_array()
        .unwrap()
        .clone()
}

fn animal(game: &ZooGame, id: u64) -> Value {
    animals(game)
        .into_iter()
        .find(|animal| animal["id"] == id)
        .unwrap()
}

/// A staffed 8×5 meadow habitat (id 1) south of the starter path.
fn meadow() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.place_habitat_rect(3, 8, 10, 12));
    ok(game.hire_keeper());
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    game
}

/// Identity, cosmetic seed and the whole movement history of one animal.
fn trajectory(game: &mut ZooGame, id: u64) -> Vec<Value> {
    let identity = |animal: &Value| {
        serde_json::json!([
            animal["name"],
            animal["sex"],
            animal["cosmetic_seed"],
            animal["x"],
            animal["y"],
            animal["activity"],
            animal["energy"]
        ])
    };
    let mut seen = vec![identity(&animal(game, id))];
    for _ in 0..60 {
        game.tick(7);
        seen.push(identity(&animal(game, id)));
    }
    seen
}

#[test]
fn other_animals_never_change_an_animals_identity_seed_or_movement() {
    let mut alone = meadow();
    ok(alone.adopt(1, "capybara".to_owned()));
    let mut crowded = meadow();
    ok(crowded.adopt(1, "capybara".to_owned()));
    ok(crowded.adopt(1, "capybara".to_owned()));
    ok(crowded.adopt(1, "capybara".to_owned()));
    let first = animal(&alone, 1);
    assert_eq!(first["habitat_id"], 1);
    assert_eq!(first["species"], "capybara");
    assert_eq!(first["life_stage"], "adult");
    assert!(first["name"].as_str().is_some_and(|name| !name.is_empty()));

    let ids: Vec<u64> = animals(&crowded)
        .iter()
        .map(|a| a["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [1, 2, 3]);
    assert_eq!(trajectory(&mut alone, 1), trajectory(&mut crowded, 1));
}

#[test]
fn animals_walk_and_rest_only_inside_a_non_rectangular_habitat() {
    let mut game = ZooGame::new();
    // An L: an 8×2 top at x 3..=10, y 8..=9 and a 3×3 leg below its west end.
    ok(game.place_habitat_outline(vec![3, 8, 11, 8, 11, 10, 6, 10, 6, 13, 3, 13, 3, 8]));
    ok(game.hire_keeper());
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    for _ in 0..4 {
        ok(game.adopt(1, "capybara".to_owned()));
    }
    let view = json(&game.snapshot_json());
    let inside: HashSet<(u64, u64)> = view["habitats"][0]["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["x"].as_u64().unwrap(), t["y"].as_u64().unwrap()))
        .collect();
    let mut activities = HashSet::new();
    let mut moved = HashSet::new();
    for _ in 0..200 {
        game.tick(5);
        for animal in animals(&game) {
            let at = (animal["x"].as_u64().unwrap(), animal["y"].as_u64().unwrap());
            assert!(inside.contains(&at), "{animal} left the enclosure");
            activities.insert(animal["activity"].as_str().unwrap().to_owned());
            moved.insert((animal["id"].as_u64().unwrap(), at));
            assert!(animal["energy"].as_u64().unwrap() <= 100);
        }
    }
    assert_eq!(
        activities,
        HashSet::from(["walking".to_owned(), "resting".to_owned()])
    );
    assert!(moved.len() > 4, "animals move between tiles");
}

#[test]
fn reshaping_keeps_every_animal_inside_the_new_fence() {
    let mut game = meadow();
    for _ in 0..3 {
        ok(game.adopt(1, "capybara".to_owned()));
    }
    game.tick(90);
    // Shrink to the western 4×5 block.
    ok(game.reshape_habitat(1, vec![3, 8, 7, 8, 7, 13, 3, 13, 3, 8]));
    for animal in animals(&game) {
        let (x, y) = (animal["x"].as_u64().unwrap(), animal["y"].as_u64().unwrap());
        assert!((3..7).contains(&x) && (8..13).contains(&y), "{animal}");
    }
    game.tick(120);
    for animal in animals(&game) {
        assert!((3..7).contains(&animal["x"].as_u64().unwrap()), "{animal}");
    }
}

#[test]
fn rendering_cadence_never_changes_animal_decisions_or_park_outcomes() {
    let mut fine = meadow();
    let mut coarse = meadow();
    for game in [&mut fine, &mut coarse] {
        ok(game.adopt(1, "capybara".to_owned()));
        ok(game.adopt(1, "capybara".to_owned()));
        ok(game.place_path(5, 7));
    }
    for _ in 0..600 {
        fine.tick(1);
    }
    coarse.tick(240);
    coarse.tick(240);
    coarse.tick(120);
    assert_eq!(fine.snapshot_json(), coarse.snapshot_json());
}

#[test]
fn names_persist_through_saves_and_replays() {
    let mut game = meadow();
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.adopt(1, "capybara".to_owned()));
    rejected(game.rename_animal(2, "   ".to_owned()));
    rejected(game.rename_animal(2, "x".repeat(25)));
    rejected(game.rename_animal(9, "Nobody".to_owned()));
    ok(game.rename_animal(2, "  Clementine ".to_owned()));
    assert_eq!(animal(&game, 2)["name"], "Clementine");
    game.tick(75);

    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    ok(loaded.load_save(&saved));
    assert_eq!(loaded.save_json(), saved);
    assert_eq!(animals(&loaded), animals(&game));
    loaded.tick(120);
    game.tick(120);
    assert_eq!(loaded.save_json(), game.save_json());
    assert_eq!(animal(&loaded, 2)["name"], "Clementine");
    // The next adoption continues the saved ID sequence.
    ok(loaded.adopt(1, "capybara".to_owned()));
    assert_eq!(animals(&loaded).last().unwrap()["id"], 3);

    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json()).unwrap();
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}

#[test]
fn guests_count_animals_where_they_actually_stand() {
    let mut game = meadow();
    ok(game.place_path(5, 7));
    ok(game.adopt(1, "capybara".to_owned()));
    for _ in 0..40 {
        game.tick(3);
        let view = json(&game.snapshot_json());
        let habitat = &view["habitats"][0];
        let animal = &view["animals"][0];
        let (ax, ay) = (animal["x"].as_i64().unwrap(), animal["y"].as_i64().unwrap());
        for spot in habitat["viewing_spots"].as_array().unwrap() {
            // A north-side viewpoint sees five tiles wide and three deep (#81).
            if spot["side"] != "north" {
                continue;
            }
            let (sx, sy) = (spot["x"].as_i64().unwrap(), spot["y"].as_i64().unwrap());
            let visible = (ax - sx).abs() <= 2 && (1..=3).contains(&(ay - sy));
            assert_eq!(
                spot["visible_animals"],
                u64::from(visible),
                "{spot} {animal}"
            );
        }
    }
}
