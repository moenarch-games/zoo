//! #108: compatible species share a habitat. Zebras and giraffes may mix; elephants and
//! zebras, or any unprofiled pair, may not, whether animals arrive by adoption, group
//! adoption, transfer or save import. Each species keeps its own social group; space, shelter,
//! enrichment, feed, water and cleanliness are shared; welfare, upkeep and feed use derive
//! from the residents and follow a removal at once.
use serde_json::{Value, json};
use zoo_core::ZooGame;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) -> Value {
    let value = parse(&result);
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn rejected(result: String) -> Value {
    let value = parse(&result);
    assert_eq!(value["ok"], false, "{value}");
    value
}

fn view(game: &ZooGame) -> Value {
    parse(&game.snapshot_json())
}

fn habitat(game: &ZooGame, id: u64) -> Value {
    view(game)["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|habitat| habitat["id"] == id)
        .unwrap()
        .clone()
}

fn resident(game: &ZooGame, habitat_id: u64, species: &str) -> Option<Value> {
    habitat(game, habitat_id)["residents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resident| resident["species"] == species)
        .cloned()
}

fn part(parts: &Value, key: &str) -> Value {
    parts
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["key"] == key)
        .unwrap()
        .clone()
}

fn animal_ids(game: &ZooGame, habitat_id: u64) -> Vec<u64> {
    view(game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|animal| animal["habitat_id"] == habitat_id)
        .map(|animal| animal["id"].as_u64().unwrap())
        .collect()
}

fn cash(game: &ZooGame) -> i64 {
    view(game)["cash_cents"].as_i64().unwrap()
}

/// A staffed park with feed and two keepers; `rects` are the habitats to build, each fenced
/// with steel mesh so giraffes may live there.
fn staffed(rects: &[(u32, u32, u32, u32)]) -> ZooGame {
    let mut game = ZooGame::new();
    for x in 2..=12 {
        ok(game.place_path(x, 7));
    }
    for (index, &(ax, ay, bx, by)) in rects.iter().enumerate() {
        ok(game.place_habitat_rect(ax, ay, bx, by));
        ok(game.upgrade_fence(index as u32 + 1, "steel_mesh".to_owned()));
    }
    ok(game.create_work_zone(0, 0, 19, 13));
    for keeper_id in 1..=2 {
        ok(game.hire_keeper());
        ok(game.assign_keeper_zone(keeper_id, 1));
    }
    for _ in 0..3 {
        ok(game.buy_animal_feed());
    }
    game
}

/// Three zebras and two giraffes in one 8×5 savanna yard (habitat #1).
fn savanna() -> ZooGame {
    let mut game = staffed(&[(3, 8, 10, 12)]);
    ok(game.adopt_group(1, "zebra".to_owned(), 3));
    ok(game.adopt_group(1, "giraffe".to_owned(), 2));
    game
}

fn feed_used(game: &ZooGame) -> u64 {
    let depot = &view(game)["animal_care_depot"];
    let carried = depot["keepers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|keeper| keeper["carrying_feed"] == true)
        .count() as u64;
    30 - depot["feed_crates"].as_u64().unwrap() - carried
}

fn upkeep(game: &ZooGame) -> i64 {
    view(game)["finance"]["current_day"]["breakdown"]["park_upkeep_expense_cents"]
        .as_i64()
        .unwrap()
}

#[test]
fn a_mixed_exhibit_runs_on_shared_care_and_differs_from_two_exhibits() {
    let mut mixed = savanna();
    let mut separate = staffed(&[(3, 8, 6, 12), (8, 8, 11, 12)]);
    ok(separate.adopt_group(1, "zebra".to_owned(), 3));
    ok(separate.adopt_group(2, "giraffe".to_owned(), 2));

    let savanna = habitat(&mixed, 1);
    assert_eq!(savanna["species"], json!(["zebra", "giraffe"]));
    assert_eq!(savanna["animals"], 5);
    // One schedule serves both species: each care category once.
    let categories = |habitat: &Value| -> Vec<Value> {
        let mut keys: Vec<Value> = habitat["care_schedule"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["category"].clone())
            .collect();
        keys.sort_by_key(ToString::to_string);
        keys
    };
    let mut union = categories(&habitat(&separate, 1));
    union.extend(categories(&habitat(&separate, 2)));
    union.sort_by_key(ToString::to_string);
    union.dedup();
    assert_eq!(categories(&savanna), union);
    for (species, animals) in [("zebra", 3), ("giraffe", 2)] {
        let resident = resident(&mixed, 1, species).unwrap();
        assert_eq!(resident["animals"], animals);
        assert_eq!(resident["social_score"], 100);
    }
    // Usable land is shared: every animal claims its own species' space.
    let space = part(&savanna["welfare_parts"], "space");
    assert!(
        space["detail"]
            .as_str()
            .unwrap()
            .ends_with("5 animals need 31"),
        "{space}"
    );

    let (upkeep_before, separate_before) = (upkeep(&mixed), upkeep(&separate));
    for _ in 0..12 * 60 {
        mixed.tick(1);
        separate.tick(1);
    }
    // One feeding round feeds both species from one crate; two exhibits need one crate each.
    let (mixed_feed, separate_feed) = (feed_used(&mixed), feed_used(&separate));
    assert!(mixed_feed > 0, "the mixed exhibit is fed with real crates");
    assert!(
        separate_feed > mixed_feed,
        "two exhibits use more crates: {separate_feed} vs {mixed_feed}"
    );
    // One habitat's base upkeep and a shorter fence: 250 + 10 steel segments × 12 per hour.
    let mixed_upkeep = upkeep(&mixed) - upkeep_before;
    let separate_upkeep = upkeep(&separate) - separate_before;
    assert_eq!(separate_upkeep - mixed_upkeep, 12 * (250 + 10 * 12));
    // The shared food pool drains with every resident.
    let savanna = habitat(&mixed, 1);
    assert!(savanna["food"].as_u64().unwrap() <= 100);
    assert!(
        view(&mixed)["habitats"][0]["care_jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|job| job["category"] == "feeding" && job["state"] == "completed"),
        "keepers fed the mixed exhibit"
    );
    assert_eq!(mixed.session_status(), "running");
}

#[test]
fn incompatible_or_overcrowded_introductions_cannot_bypass_validation() {
    let mut game = savanna();
    let before = (cash(&game), animal_ids(&game, 1));

    // An authored incompatible pair is refused with its reason, nothing spent.
    let preview = parse(&game.evaluate_adoption(1, "elephant".to_owned(), 1));
    assert_eq!(preview["ok"], false);
    let zebra_pairing = preview["pairings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pairing| pairing["species"] == json!(["elephant", "zebra"]))
        .unwrap()
        .clone();
    assert_eq!(zebra_pairing["compatible"], false);
    assert!(
        zebra_pairing["reason"]
            .as_str()
            .unwrap()
            .contains("trample")
    );
    let refused = rejected(game.adopt(1, "elephant".to_owned()));
    assert!(
        refused["message"].as_str().unwrap().contains("trample"),
        "{refused}"
    );
    // An unprofiled pair is unsupported.
    let refused = rejected(game.adopt_group(1, "flamingo".to_owned(), 3));
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("No mixed-exhibit profile"),
        "{refused}"
    );
    // A group larger than the room left is refused whole (capacity 12, 5 living there).
    let refused = rejected(game.adopt_group(1, "zebra".to_owned(), 8));
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("room for 7 more"),
        "{refused}"
    );
    assert_eq!((cash(&game), animal_ids(&game, 1)), before);

    // Transfers pass the same rules: an elephant cannot move in with the zebras.
    ok(game.place_habitat_rect(12, 8, 17, 12));
    ok(game.upgrade_fence(2, "steel_mesh".to_owned()));
    ok(game.adopt(2, "elephant".to_owned()));
    let elephant = animal_ids(&game, 2)[0];
    let refused = rejected(game.transfer_animal(elephant as u32, 1));
    assert!(refused["message"].as_str().unwrap().contains("trample"));
    assert_eq!(animal_ids(&game, 2), [elephant]);
    // ... and a zebra cannot move in with the elephant.
    let zebra = animal_ids(&game, 1)[0];
    rejected(game.transfer_animal(zebra as u32, 2));
    let save = game.save_json();

    // Save import: a hand-edited mix, an overcrowded habitat or a missing profile is refused
    // and the running park stays as it was.
    let base = parse(&save);
    let mut incompatible = base.clone();
    {
        let habitats = incompatible["park"]["habitats"].as_array_mut().unwrap();
        let (savanna, yard) = habitats.split_at_mut(1);
        let elephant_resident = yard[0]["residents"][0].clone();
        let elephant_animal = yard[0]["animals"][0].clone();
        yard[0]["residents"] = json!([]);
        yard[0]["animals"] = json!([]);
        let mut moved = elephant_animal;
        moved["position"] = savanna[0]["animals"][0]["position"].clone();
        savanna[0]["residents"]
            .as_array_mut()
            .unwrap()
            .push(elephant_resident);
        savanna[0]["animals"].as_array_mut().unwrap().push(moved);
    }
    let mut unprofiled = base.clone();
    unprofiled["content"]["species_compatibility"] = json!([]);
    let mut overcrowded = base.clone();
    {
        let animals = overcrowded["park"]["habitats"][0]["animals"]
            .as_array_mut()
            .unwrap();
        let mut next = base["park"]["next_animal_id"].as_u64().unwrap();
        while animals.len() < 13 {
            let mut copy = animals[0].clone();
            copy["id"] = next.into();
            next += 1;
            animals.push(copy);
        }
        overcrowded["park"]["next_animal_id"] = next.into();
    }
    let mut orphan = base.clone();
    orphan["park"]["habitats"][0]["residents"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let snapshot = game.snapshot_json();
    for (case, document) in [
        ("incompatible", incompatible),
        ("unprofiled", unprofiled),
        ("overcrowded", overcrowded),
        ("orphan", orphan),
    ] {
        let result = rejected(game.load_save(&document.to_string()));
        assert_eq!(result["code"], "invalid_park", "{case}: {result}");
        assert_eq!(game.save_json(), save, "{case}");
        assert_eq!(game.snapshot_json(), snapshot, "{case}");
    }
}

#[test]
fn one_species_never_completes_another_social_group() {
    let mut game = staffed(&[(3, 8, 10, 12)]);
    ok(game.adopt(1, "zebra".to_owned()));
    ok(game.adopt_group(1, "giraffe".to_owned(), 2));
    // Three animals live together, but only one is a zebra.
    let zebras = resident(&game, 1, "zebra").unwrap();
    assert_eq!(zebras["social_score"], 33);
    assert_eq!(
        part(&zebras["welfare_parts"], "social")["detail"],
        "1 of at least 3 together"
    );
    assert_eq!(resident(&game, 1, "giraffe").unwrap()["social_score"], 100);
    let savanna = habitat(&game, 1);
    assert_eq!(savanna["social_score"], 33);
    assert_eq!(
        savanna["welfare_status"],
        "Zebra group needs at least 3 animals"
    );
    let social = part(&savanna["welfare_parts"], "social");
    assert_eq!(social["detail"], "Zebra: 1 of at least 3 together");
    game.tick(1);
    let alert = view(&game)["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|alert| alert["code"] == "unsuitable_group")
        .cloned()
        .expect("the zebra's group is reported");
    assert!(
        alert["message"].as_str().unwrap().contains("1 Zebra"),
        "{alert}"
    );
    // Their welfare diverges: the lone zebra's target is lower than the giraffes'.
    let (zebra, giraffe) = (
        resident(&game, 1, "zebra").unwrap(),
        resident(&game, 1, "giraffe").unwrap(),
    );
    assert!(zebra["welfare_target"].as_u64() < giraffe["welfare_target"].as_u64());
    let animal = view(&game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|animal| animal["species"] == "zebra")
        .unwrap()
        .clone();
    assert_eq!(animal["welfare"], zebra["welfare"]);
}

#[test]
fn shared_resources_reconcile_to_the_residents_and_follow_a_removal_while_paused() {
    let mut game = savanna();
    ok(game.add_shelter(1)); // A basic shelter: four places.
    let savanna = habitat(&game, 1);
    let shelter = part(&savanna["welfare_parts"], "shelter");
    assert!(
        shelter["detail"]
            .as_str()
            .unwrap()
            .contains("Reachable shelter for 4 of 5 animals"),
        "{shelter}"
    );
    assert!(shelter["score"].as_u64() < Some(100));

    // The preview of one more giraffe reconciles with what adopting it does.
    let preview = parse(&game.evaluate_adoption(1, "giraffe".to_owned(), 1));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["cost_cents"], 65_000);
    let effect = &preview["destination"];
    assert_eq!(effect["shelter"]["provided"], 4);
    assert_eq!(effect["shelter"]["needed_before"], 5);
    assert_eq!(effect["shelter"]["needed_after"], 6);
    assert_eq!(effect["space"]["needed_after"], 39);
    assert_eq!(effect["needs_per_hour_before"], 20);
    assert_eq!(effect["needs_per_hour_after"], 24);
    assert_eq!(effect["upkeep_cents_per_hour_after"], 6 * 125);
    assert!(
        preview["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning == "Shared shelter: places for 4 of 6 animals"),
        "{preview}"
    );
    let target_after = effect["residents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resident| resident["species"] == "giraffe")
        .unwrap()["welfare_target_after"]
        .clone();
    let cash_before = cash(&game);
    ok(game.adopt(1, "giraffe".to_owned()));
    assert_eq!(cash(&game), cash_before - 65_000);
    assert_eq!(
        resident(&game, 1, "giraffe").unwrap()["welfare_target"],
        target_after
    );

    // Removing animals updates residents, shared places, welfare and upkeep at once, with no
    // simulated time passing.
    let giraffes: Vec<u64> = view(&game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|animal| animal["species"] == "giraffe")
        .map(|animal| animal["id"].as_u64().unwrap())
        .collect();
    let minute = view(&game)["minute_of_day"].clone();
    for id in &giraffes[..2] {
        ok(game.rehome_animal(*id as u32));
    }
    let savanna = habitat(&game, 1);
    assert_eq!(view(&game)["minute_of_day"], minute);
    assert_eq!(savanna["animals"], 4);
    let shelter = part(&savanna["welfare_parts"], "shelter");
    assert!(
        shelter["detail"]
            .as_str()
            .unwrap()
            .contains("Reachable shelter for 4 of 4 animals"),
        "{shelter}"
    );
    let preview = parse(&game.evaluate_adoption(1, "zebra".to_owned(), 1));
    assert_eq!(
        preview["destination"]["upkeep_cents_per_hour_before"],
        4 * 125
    );
    assert_eq!(preview["destination"]["needs_per_hour_before"], 16);
    // The last giraffe leaving removes its species; the welfare is the zebras' alone.
    ok(game.rehome_animal(giraffes[2] as u32));
    let savanna = habitat(&game, 1);
    assert_eq!(savanna["species"], json!(["zebra"]));
    assert_eq!(
        savanna["welfare"],
        resident(&game, 1, "zebra").unwrap()["welfare"]
    );

    // One more hour of upkeep charges exactly the animals still living here.
    let mut fewer = savanna_with_shelter();
    let mut all = savanna_with_shelter();
    let zebra = animal_ids(&fewer, 1)[0];
    ok(fewer.rehome_animal(zebra as u32));
    let (fewer_before, all_before) = (upkeep(&fewer), upkeep(&all));
    fewer.tick(60);
    all.tick(60);
    assert_eq!(
        (upkeep(&all) - all_before) - (upkeep(&fewer) - fewer_before),
        125
    );
}

fn savanna_with_shelter() -> ZooGame {
    let mut game = savanna();
    ok(game.add_shelter(1));
    game
}

#[test]
fn transfers_keep_identity_and_update_both_habitats() {
    let mut game = staffed(&[(3, 8, 10, 12), (12, 8, 15, 12)]);
    ok(game.adopt_group(1, "zebra".to_owned(), 3));
    ok(game.adopt_group(2, "giraffe".to_owned(), 2));
    let giraffe = animal_ids(&game, 2)[0];
    ok(game.rename_animal(giraffe as u32, "Tallulah".to_owned()));
    let before = view(&game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|animal| animal["id"] == giraffe)
        .unwrap()
        .clone();
    let preview = parse(&game.evaluate_transfer(giraffe as u32, 1));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["source"]["animals_after"], 1);
    assert!(
        preview["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("1 Giraffe stay behind")),
        "{preview}"
    );
    ok(game.transfer_animal(giraffe as u32, 1));
    let after = view(&game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|animal| animal["id"] == giraffe)
        .unwrap()
        .clone();
    assert_eq!(after["habitat_id"], 1);
    for key in ["name", "sex", "cosmetic_seed", "arrived_day", "species"] {
        assert_eq!(after[key], before[key], "{key}");
    }
    assert_eq!(after["name"], "Tallulah");
    assert_eq!(habitat(&game, 1)["species"], json!(["zebra", "giraffe"]));
    assert_eq!(habitat(&game, 2)["animals"], 1);
    // The transferred giraffe brings its group's welfare into the new habitat.
    assert_eq!(
        resident(&game, 1, "giraffe").unwrap()["welfare"],
        resident(&game, 2, "giraffe").unwrap()["welfare"]
    );
    rejected(game.transfer_animal(giraffe as u32, 1));
}

/// Commands after the save point: a transfer, a group adoption, a rehoming and time.
fn continue_mixed(game: &mut ZooGame) {
    game.tick(97);
    let giraffe = view(game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|animal| animal["species"] == "giraffe")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    ok(game.transfer_animal(giraffe as u32, 2));
    ok(game.adopt_group(1, "zebra".to_owned(), 2));
    let zebra = animal_ids(game, 1)[0];
    ok(game.rehome_animal(zebra as u32));
    for _ in 0..10 {
        game.tick(53);
    }
}

#[test]
fn save_and_load_preserve_residents_and_deterministic_outcomes() {
    let mut game = staffed(&[(3, 8, 10, 12), (12, 8, 15, 12)]);
    ok(game.adopt_group(1, "zebra".to_owned(), 3));
    ok(game.adopt_group(1, "giraffe".to_owned(), 2));
    ok(game.add_shelter(1));
    game.tick(211);
    let saved = game.save_json();

    let mut reloaded = ZooGame::new();
    ok(reloaded.load_save(&saved));
    assert_eq!(
        reloaded.save_json(),
        saved,
        "a load restores the park exactly"
    );
    let residents = &parse(&saved)["park"]["habitats"][0]["residents"];
    assert_eq!(residents[0]["species"]["key"], "zebra");
    assert_eq!(residents[1]["species"]["key"], "giraffe");
    assert_eq!(
        view(&reloaded)["animals"],
        view(&game)["animals"],
        "identities, names and positions survive"
    );

    continue_mixed(&mut game);
    continue_mixed(&mut reloaded);
    assert_eq!(reloaded.save_json(), game.save_json());

    // The same commands from scratch reach the same park: no hidden randomness.
    let mut again = staffed(&[(3, 8, 10, 12), (12, 8, 15, 12)]);
    ok(again.adopt_group(1, "zebra".to_owned(), 3));
    ok(again.adopt_group(1, "giraffe".to_owned(), 2));
    ok(again.add_shelter(1));
    again.tick(211);
    continue_mixed(&mut again);
    assert_eq!(again.save_json(), game.save_json());
}

#[test]
fn group_adoptions_transfers_and_rehoming_replay_from_the_session_log() {
    let mut game = staffed(&[(3, 8, 10, 12), (12, 8, 15, 12)]);
    let content = game.content_json();
    ok(game.adopt_group(1, "zebra".to_owned(), 3));
    ok(game.adopt_group(1, "giraffe".to_owned(), 2));
    rejected(game.adopt_group(1, "elephant".to_owned(), 2));
    game.tick(30);
    let giraffe = animal_ids(&game, 1)[3];
    ok(game.transfer_animal(giraffe as u32, 2));
    ok(game.rehome_animal(animal_ids(&game, 1)[0] as u32));
    game.tick(30);
    let replayed = ZooGame::replay(&content, &game.diagnostics_json()).expect("replays");
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}

#[test]
fn compatibility_profiles_are_validated_content() {
    let catalog = || parse(&ZooGame::new().content_json());
    for (case, edit) in [
        (
            "unknown",
            json!({"species": ["zebra", "unicorn"], "compatible": true, "reason": "x"}),
        ),
        (
            "same",
            json!({"species": ["zebra", "zebra"], "compatible": true, "reason": "x"}),
        ),
        (
            "duplicate",
            json!({"species": ["zebra", "giraffe"], "compatible": false, "reason": "x"}),
        ),
        (
            "empty reason",
            json!({"species": ["capybara", "zebra"], "compatible": true, "reason": ""}),
        ),
    ] {
        let mut definition = catalog();
        definition["species_compatibility"]
            .as_array_mut()
            .unwrap()
            .push(edit);
        assert!(
            ZooGame::from_content_json(&definition.to_string()).is_err(),
            "{case}"
        );
    }
}

const PROOF_PARK: &str = "tests/fixtures/mixed-species-park.json";

/// The park the browser proof (apps/web/scripts/mixed-species-proof.mjs) opens: three zebras
/// and one basic shelter in a staffed 8×5 steel-mesh yard (habitat #1), paused at a fixed
/// time. It must stay a current, loadable save; rewrite it with
/// `ZOO_WRITE_MIXED_SPECIES_PARK=1`.
#[test]
fn the_mixed_species_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_MIXED_SPECIES_PARK").is_some() {
        let mut game = staffed(&[(3, 8, 10, 12)]);
        ok(game.adopt_group(1, "zebra".to_owned(), 3));
        ok(game.add_shelter(1));
        game.tick(30);
        let value = parse(&game.save_json());
        std::fs::write(
            PROOF_PARK,
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
    let fixture = std::fs::read_to_string(PROOF_PARK).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    assert_eq!(parse(&game.save_json()), parse(&fixture));
    // Two giraffes join the zebras, overfilling the shared shelter; a large shelter fixes it.
    let preview = parse(&game.evaluate_adoption(1, "giraffe".to_owned(), 2));
    assert_eq!(preview["ok"], true, "{preview}");
    assert!(
        preview["warnings"]
            .as_array()
            .unwrap()
            .contains(&json!("Shared shelter: places for 4 of 5 animals")),
        "{preview}"
    );
    ok(game.adopt_group(1, "giraffe".to_owned(), 2));
    assert_eq!(
        part(&habitat(&game, 1)["welfare_parts"], "shelter")["met"],
        false
    );
    ok(game.place_habitat_object(1, "large_shelter".to_owned(), 3, 12));
    assert_eq!(
        part(&habitat(&game, 1)["welfare_parts"], "shelter")["met"],
        true
    );
}
