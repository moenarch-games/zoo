//! #113: utility capacity, habitat climate control and facility service outages.
//!
//! Penguins draw one unit of pool water and one of climate control each. A habitat is
//! supplied only by working plants a staff path joins to its service gate within their
//! coverage; capacity is handed out in a fixed, inspectable order; failed plants supply
//! nothing until a mechanic repairs them or they are replaced. A shortage warns for an hour
//! before it touches welfare, and supply restored clears it at once.
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
        .cloned()
        .unwrap()
}

fn supply(game: &ZooGame, habitat_id: u64, kind: &str) -> Value {
    habitat(game, habitat_id)["utilities"][kind].clone()
}

fn plant(game: &ZooGame, id: u64) -> Value {
    view(game)["concessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stand| stand["id"] == id)
        .cloned()
        .unwrap()
}

fn alerts(game: &ZooGame, code: &str) -> Vec<Value> {
    view(game)["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|alert| alert["code"] == code)
        .cloned()
        .collect()
}

fn part(game: &ZooGame, habitat_id: u64, key: &str) -> Option<Value> {
    habitat(game, habitat_id)["welfare_parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["key"] == key)
        .cloned()
}

fn run_until(game: &mut ZooGame, limit: u32, done: impl Fn(&ZooGame) -> bool) -> u32 {
    for minute in 0..limit {
        if done(game) {
            return minute;
        }
        game.tick(1);
    }
    assert!(
        done(game),
        "the condition never held within {limit} minutes"
    );
    limit
}

const CLIMATE: u32 = 1;
const WATER: u32 = 2;

/// A public path along y = 7 with two 5×4 habitats south of it (#1 at x 3..=7, #2 at x
/// 9..=13), their gates on the path. Four penguins live in habitat #1.
fn penguins() -> ZooGame {
    let mut game = ZooGame::new();
    for x in 1..=16 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 7, 11));
    ok(game.place_habitat_rect(9, 8, 13, 11));
    staff(&mut game);
    ok(game.adopt_group(1, "penguin".to_owned(), 4));
    game
}

/// One park-wide zone with a keeper and feed, so habitats may adopt.
fn staff(game: &mut ZooGame) {
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
}

/// Demolishes a path tile as soon as nobody stands on it.
fn demolish(game: &mut ZooGame, x: u32, y: u32) {
    for _ in 0..120 {
        if parse(&game.bulldoze(x, y))["ok"] == true {
            return;
        }
        game.tick(1);
    }
    panic!("({x}, {y}) never cleared");
}

/// The penguin park with climate plant #1 at (8, 6) and water pump #2 at (10, 6).
fn supplied() -> ZooGame {
    let mut game = penguins();
    ok(game.place_concession(8, 6, "climate_plant".to_owned()));
    ok(game.place_concession(10, 6, "water_pump".to_owned()));
    game
}

fn failed(game: &ZooGame, id: u64) -> bool {
    plant(game, id)["service_state"] == "failed"
}

#[test]
fn a_supported_habitat_works_degrades_on_failure_and_recovers_after_repair() {
    let mut game = penguins();
    // Nothing built yet: short, with the reason and the fix, and a warning hour first.
    let climate = supply(&game, 1, "climate");
    assert_eq!(climate["demand"], 4);
    assert_eq!(climate["supplied"], 0);
    assert_eq!(climate["status"], "short");
    assert_eq!(climate["reason"], "none_built");
    assert!(climate["fix"].as_str().unwrap().contains("climate plant"));
    let warning = alerts(&game, "climate_shortage");
    assert_eq!(warning.len(), 1);
    assert_eq!(warning[0]["action"], "build_climate_plant");
    assert_eq!(supply(&game, 2, "climate")["status"], "not_needed");
    assert!(part(&game, 1, "climate_control").is_none());
    run_until(&mut game, 61, |game| {
        supply(game, 1, "climate")["status"] == "degraded"
    });
    let degraded = part(&game, 1, "climate_control").expect("a degraded welfare part");
    assert_eq!(degraded["score"], 40, "no supply scores the floor");
    let low_target = habitat(&game, 1)["welfare_target"].as_u64().unwrap();

    // Sufficient service: fully supplied at once, the part gone and the target higher.
    ok(game.place_concession(8, 6, "climate_plant".to_owned()));
    ok(game.place_concession(10, 6, "water_pump".to_owned()));
    for kind in ["climate", "water"] {
        let supply = supply(&game, 1, kind);
        assert_eq!(supply["status"], "supplied", "{supply}");
        assert_eq!(supply["supplied"], 4);
        assert_eq!(supply["rank"], 1);
    }
    assert_eq!(
        supply(&game, 1, "climate")["draws"],
        json!([{ "concession_id": CLIMATE, "units": 4 }])
    );
    assert!(part(&game, 1, "climate_control").is_none());
    assert!(alerts(&game, "climate_shortage").is_empty());
    assert!(habitat(&game, 1)["welfare_target"].as_u64().unwrap() > low_target);
    let utility = &plant(&game, u64::from(CLIMATE))["utility"];
    assert_eq!(utility["allocated"], 4);
    assert_eq!(utility["capacity"], 8);
    assert_eq!(
        utility["consumers"],
        json!([{ "habitat_id": 1, "units": 4, "rank": 1, "priority": "normal" }])
    );

    // Equipment wears; with no mechanic it fails, supplies nothing and says why.
    run_until(&mut game, 24 * 60, |game| failed(game, u64::from(CLIMATE)));
    let climate = supply(&game, 1, "climate");
    assert_eq!(climate["supplied"], 0);
    assert_eq!(climate["reason"], "failed");
    let shortage = alerts(&game, "climate_shortage");
    assert_eq!(shortage[0]["action"], "hire_mechanic");
    assert_eq!(shortage[0]["focus"], json!({ "x": 8, "y": 6 }));
    assert!(alerts(&game, "facility_failed").iter().any(|alert| {
        alert["message"]
            .as_str()
            .unwrap()
            .contains("has failed and supplies nothing")
    }));
    assert_eq!(
        plant(&game, u64::from(CLIMATE))["utility"]["working"],
        false
    );
    run_until(&mut game, 61, |game| {
        supply(game, 1, "climate")["status"] == "degraded"
    });
    assert!(part(&game, 1, "climate_control").is_some());

    // Intervention: a mechanic repairs it and supply returns; the effect clears at once.
    ok(game.hire_mechanic());
    run_until(&mut game, 240, |game| !failed(game, u64::from(CLIMATE)));
    assert_eq!(supply(&game, 1, "climate")["status"], "supplied");
    assert!(part(&game, 1, "climate_control").is_none());
    assert!(alerts(&game, "climate_shortage").is_empty());
}

#[test]
fn decorative_unconnected_and_out_of_range_equipment_provides_no_hidden_service() {
    let mut game = penguins();
    // Furnishings are not equipment.
    ok(game.add_shelter(1));
    assert_eq!(supply(&game, 1, "climate")["reason"], "none_built");
    // A plant on a path island that does not reach the gate supplies nothing.
    ok(game.place_path(16, 2));
    ok(game.place_concession(16, 1, "climate_plant".to_owned()));
    let climate = supply(&game, 1, "climate");
    assert_eq!(climate["reason"], "unconnected");
    assert_eq!(climate["reachable"], json!([]));
    assert_eq!(alerts(&game, "climate_shortage")[0]["action"], "build_path");
    let utility = &plant(&game, 1)["utility"];
    assert_eq!(utility["allocated"], 0);
    assert!(utility["status"].as_str().unwrap().starts_with("Idle"));
    // Guest paths are not needed: a staff-only lane is enough to connect it.
    ok(game.build_path_stroke(vec![16, 3, 16, 6], 1, "staff".to_owned()));
    assert_eq!(supply(&game, 1, "climate")["status"], "supplied");
    assert!(alerts(&game, "destination_isolated").is_empty());
    // Plants must touch a path at all.
    let refused = rejected(game.place_concession(18, 1, "water_pump".to_owned()));
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("must touch a path mechanics can walk")
    );

    // Coverage counts path tiles: with a coverage of 3 the far habitat is out of range.
    let mut catalog = parse(&ZooGame::new().content_json());
    for facility in catalog["facilities"].as_array_mut().unwrap() {
        if facility["key"] == "climate_plant" {
            facility["utility"]["coverage_tiles"] = json!(3);
        }
    }
    let mut game = ZooGame::from_content_json(&catalog.to_string()).unwrap();
    for x in 1..=16 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 7, 11));
    staff(&mut game);
    ok(game.adopt_group(1, "penguin".to_owned(), 4));
    ok(game.place_concession(16, 6, "climate_plant".to_owned()));
    assert_eq!(supply(&game, 1, "climate")["reason"], "unconnected");
}

#[test]
fn simultaneous_consumers_never_exceed_capacity_and_priority_is_deterministic() {
    let mut game = supplied();
    ok(game.adopt_group(1, "penguin".to_owned(), 2));
    ok(game.adopt_group(2, "penguin".to_owned(), 4));
    // Ten penguins need ten units from an eight-unit plant: habitat #1 comes first by ID.
    let first = supply(&game, 1, "climate");
    let second = supply(&game, 2, "climate");
    assert_eq!(
        (first["supplied"].clone(), first["rank"].clone()),
        (json!(6), json!(1))
    );
    assert_eq!(
        (second["supplied"].clone(), second["rank"].clone()),
        (json!(2), json!(2))
    );
    assert_eq!(second["reason"], "overloaded");
    let utility = &plant(&game, u64::from(CLIMATE))["utility"];
    assert_eq!(utility["allocated"], 8);
    assert_eq!(utility["overloaded"], true);
    assert_eq!(utility["short_habitats"], json!([2]));
    let overloaded = alerts(&game, "utility_overloaded");
    assert_eq!(overloaded.len(), 1);
    assert!(
        overloaded[0]["message"]
            .as_str()
            .unwrap()
            .contains("habitat #2")
    );
    // The ten units of water fit in the twelve-unit pump.
    assert_eq!(supply(&game, 2, "water")["status"], "supplied");

    // Priority reorders it, and setting it again changes nothing.
    ok(game.set_habitat_utility_priority(2, "high".to_owned()));
    assert_eq!(supply(&game, 2, "climate")["supplied"], 4);
    assert_eq!(supply(&game, 2, "climate")["rank"], 1);
    assert_eq!(supply(&game, 1, "climate")["supplied"], 4);
    assert_eq!(supply(&game, 1, "climate")["reason"], "overloaded");
    let again = ok(game.set_habitat_utility_priority(2, "high".to_owned()));
    assert!(again["message"].as_str().unwrap().contains("already"));
    rejected(game.set_habitat_utility_priority(2, "urgent".to_owned()));
    rejected(game.set_habitat_utility_priority(9, "high".to_owned()));

    // Allocation holds every minute, and a twin park allocates identically.
    let mut twin = supplied();
    ok(twin.adopt_group(1, "penguin".to_owned(), 2));
    ok(twin.adopt_group(2, "penguin".to_owned(), 4));
    ok(twin.set_habitat_utility_priority(2, "high".to_owned()));
    for _ in 0..180 {
        game.tick(1);
        twin.tick(1);
        let allocated: u64 = [1, 2]
            .iter()
            .map(|id| supply(&game, *id, "climate")["supplied"].as_u64().unwrap())
            .sum();
        assert!(allocated <= 8);
    }
    let utilities = |game: &ZooGame| {
        view(game)["habitats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|habitat| habitat["utilities"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(utilities(&game), utilities(&twin));
    // A second plant ends the overload.
    ok(game.place_concession(12, 6, "climate_plant".to_owned()));
    assert_eq!(supply(&game, 1, "climate")["status"], "supplied");
    assert!(alerts(&game, "utility_overloaded").is_empty());
}

fn ledger(game: &ZooGame) -> Value {
    view(game)["finance"]["current_day"]["breakdown"].clone()
}

fn cents(ledger: &Value, field: &str) -> i64 {
    ledger[field].as_i64().unwrap()
}

#[test]
fn repairs_operating_costs_and_replacements_reconcile_to_the_ledger() {
    let mut game = supplied();
    // Run until both plants are past due, crossing day boundaries; every minute the cash moves
    // exactly by the ledger.
    let mut before = view(&game);
    let mut operation_total = 0;
    for _ in 0..(24 * 60) {
        game.tick(1);
        let now = view(&game);
        let delta = |snapshot: &Value, field: &str| -> i64 {
            snapshot["finance"]["current_day"][field].as_i64().unwrap()
        };
        let (income, expenses, operation) = if now["day"] == before["day"] {
            (
                delta(&now, "income_cents") - delta(&before, "income_cents"),
                delta(&now, "expenses_cents") - delta(&before, "expenses_cents"),
                cents(
                    &now["finance"]["current_day"]["breakdown"],
                    "utility_operation_expense_cents",
                ) - cents(
                    &before["finance"]["current_day"]["breakdown"],
                    "utility_operation_expense_cents",
                ),
            )
        } else {
            let previous = &now["finance"]["previous_day"];
            (
                previous["income_cents"].as_i64().unwrap() - delta(&before, "income_cents")
                    + delta(&now, "income_cents"),
                previous["expenses_cents"].as_i64().unwrap() - delta(&before, "expenses_cents")
                    + delta(&now, "expenses_cents"),
                cents(&previous["breakdown"], "utility_operation_expense_cents")
                    - cents(
                        &before["finance"]["current_day"]["breakdown"],
                        "utility_operation_expense_cents",
                    )
                    + cents(
                        &now["finance"]["current_day"]["breakdown"],
                        "utility_operation_expense_cents",
                    ),
            )
        };
        assert_eq!(
            now["cash_cents"].as_i64().unwrap() - before["cash_cents"].as_i64().unwrap(),
            income - expenses,
            "cash moves exactly by the ledger"
        );
        // Operating cost is the working plants' hourly rate, charged on the hour.
        let working: i64 = [CLIMATE, WATER]
            .iter()
            .filter(|id| {
                before["concessions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|stand| stand["id"] == **id && stand["service_state"] != "failed")
            })
            .map(|id| if *id == CLIMATE { 90 } else { 40 })
            .sum();
        assert!(
            operation == 0 || operation == working,
            "{operation} vs {working}"
        );
        operation_total += operation;
        before = now;
    }
    assert!(operation_total > 0);
    assert!(failed(&game, u64::from(CLIMATE)) && failed(&game, u64::from(WATER)));
    // A failed plant costs nothing to run.
    let quiet = cents(&ledger(&game), "utility_operation_expense_cents");
    game.tick(60);
    if view(&game)["minute_of_day"].as_u64().unwrap() >= 60 {
        assert_eq!(
            cents(&ledger(&game), "utility_operation_expense_cents"),
            quiet
        );
    }

    // A replacement costs exactly the build price, restores the plant and withdraws its task.
    let cash = view(&game)["cash_cents"].as_i64().unwrap();
    let replaced = cents(&ledger(&game), "utility_replacement_expense_cents");
    ok(game.replace_utility(CLIMATE));
    assert_eq!(view(&game)["cash_cents"].as_i64().unwrap(), cash - 16_000);
    assert_eq!(
        cents(&ledger(&game), "utility_replacement_expense_cents"),
        replaced + 16_000
    );
    assert_eq!(plant(&game, u64::from(CLIMATE))["condition"], 100);
    assert!(
        !view(&game)["maintenance"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["concession_id"] == CLIMATE)
    );
    assert_eq!(supply(&game, 1, "climate")["status"], "supplied");
    let again = rejected(game.replace_utility(CLIMATE));
    assert!(again["message"].as_str().unwrap().contains("good order"));
    rejected(game.replace_utility(99));

    // A mechanic's repair costs exactly the repair price.
    let repairs = cents(&ledger(&game), "maintenance_repair_expense_cents");
    ok(game.hire_mechanic());
    run_until(&mut game, 240, |game| !failed(game, u64::from(WATER)));
    assert_eq!(
        cents(&ledger(&game), "maintenance_repair_expense_cents"),
        repairs + 2_500
    );
}

/// A failed climate plant with a mechanic walking to it.
fn mid_repair() -> ZooGame {
    let mut game = supplied();
    run_until(&mut game, 24 * 60, |game| failed(game, u64::from(CLIMATE)));
    ok(game.hire_mechanic());
    run_until(&mut game, 30, |game| {
        view(game)["maintenance"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["concession_id"] == CLIMATE && task["assigned_mechanic_id"] == 1)
    });
    game
}

#[test]
fn removing_a_plant_editing_access_and_reloading_mid_repair_stay_valid_and_explained() {
    // Editing access: cutting the path between plant and gate disconnects it at once.
    let mut game = supplied();
    let gate = habitat(&game, 1)["service_gate"]["access"].clone();
    assert_eq!(gate["y"], 7);
    let gate_x = gate["x"].as_u64().unwrap() as u32;
    let cut = gate_x + 1;
    assert!(cut < 8, "the cut lies between the gate and the plant");
    demolish(&mut game, cut, 7);
    let climate = supply(&game, 1, "climate");
    assert_eq!(climate["reason"], "unconnected", "{climate}");
    assert_eq!(climate["draws"], json!([]));
    assert_eq!(
        plant(&game, u64::from(CLIMATE))["utility"]["consumers"],
        json!([])
    );
    ok(game.place_path(cut, 7));
    assert_eq!(supply(&game, 1, "climate")["status"], "supplied");

    // Save and load mid-repair: the same assignment, the same allocation, the same future.
    let mut game = mid_repair();
    assert_eq!(supply(&game, 1, "climate")["reason"], "failed");
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    ok(loaded.load_save(&saved));
    assert_eq!(parse(&loaded.save_json()), parse(&saved));
    assert_eq!(view(&loaded)["habitats"], view(&game)["habitats"]);
    let minutes = run_until(&mut loaded, 240, |game| !failed(game, u64::from(CLIMATE)));
    game.tick(minutes);
    assert!(!failed(&game, u64::from(CLIMATE)));
    assert_eq!(
        parse(&loaded.save_json())["park"],
        parse(&game.save_json())["park"]
    );
    assert_eq!(supply(&loaded, 1, "climate")["status"], "supplied");

    // Removing a plant mid-repair withdraws its task and frees the mechanic; the habitat says
    // why it is short.
    let mut game = mid_repair();
    let task = view(&game)["maintenance"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["concession_id"] == CLIMATE)
        .unwrap()["id"]
        .clone();
    ok(game.bulldoze(8, 6));
    let snapshot = view(&game);
    assert!(
        !snapshot["maintenance"]
            .as_array()
            .unwrap()
            .iter()
            .any(|open| open["id"] == task)
    );
    assert!(
        !game
            .save_json()
            .contains(&format!("\"target_maintenance_id\":{task}"))
    );
    let climate = supply(&game, 1, "climate");
    assert_eq!(climate["reason"], "none_built");
    assert_eq!(climate["draws"], json!([]));
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    ok(loaded.load_save(&saved));
    loaded.tick(30);
    assert_eq!(loaded.session_status(), "running");
}

#[test]
fn a_tampered_allocation_is_refused_on_load() {
    let game = supplied();
    let mut save = parse(&game.save_json());
    save["park"]["habitats"][0]["utilities"]["climate"]["draws"][0]["units"] = json!(40);
    save["park"]["habitats"][0]["utilities"]["climate"]["supplied"] = json!(40);
    let refused = parse(&ZooGame::new().load_save(&save.to_string()));
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["code"], "invalid_park");
}

#[test]
fn content_must_supply_every_utility_a_species_draws() {
    let mut catalog = parse(&ZooGame::new().content_json());
    catalog["facilities"]
        .as_array_mut()
        .unwrap()
        .retain(|facility| facility["key"] != "climate_plant");
    let error = ZooGame::from_content_json(&catalog.to_string())
        .err()
        .expect("penguins need a climate plant");
    assert!(error.contains("no facility supplies it"), "{error}");

    let mut catalog = parse(&ZooGame::new().content_json());
    for facility in catalog["facilities"].as_array_mut().unwrap() {
        if facility["key"] == "water_pump" {
            facility["utility"]["kind"] = json!("climate");
        }
    }
    let error = ZooGame::from_content_json(&catalog.to_string())
        .err()
        .expect("a pump supplies water");
    assert!(error.contains("supplies water"), "{error}");
}

#[test]
fn the_utilities_overlay_grades_coverage_plants_and_habitats() {
    let mut game = supplied();
    let overlay = parse(&game.overlay_json("utilities"));
    assert_eq!(overlay["kind"], "utilities");
    assert_eq!(overlay["bad"], 0);
    assert!(
        overlay["good"].as_u64().unwrap() > 10,
        "coverage is painted"
    );
    run_until(&mut game, 24 * 60, |game| failed(game, u64::from(CLIMATE)));
    let overlay = parse(&game.overlay_json("utilities"));
    assert!(
        overlay["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |item| item["subject"] == json!({ "kind": "concession", "id": CLIMATE })
                    && item["level"] == "bad"
            )
    );
}

const PROOF_PARK: &str = "tests/fixtures/utilities-park.json";

/// The park the browser proof (apps/web/scripts/utilities-proof.mjs) opens: four penguins in
/// habitat #1 supplied by climate plant #1 and water pump #2. The climate plant is one wear
/// step from failing and no mechanic is hired; the pump was built later and is sound. It must
/// stay a current, loadable save; rewrite it with `ZOO_WRITE_UTILITIES_PARK=1`.
#[test]
fn the_utilities_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_UTILITIES_PARK").is_some() {
        let mut game = penguins();
        ok(game.place_concession(8, 6, "climate_plant".to_owned()));
        game.tick(6 * 60);
        ok(game.place_concession(10, 6, "water_pump".to_owned()));
        run_until(&mut game, 24 * 60, |game| {
            plant(game, u64::from(CLIMATE))["condition"]
                .as_u64()
                .unwrap()
                <= 22
        });
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
    // The proof's journey: supplied, an outage and its alert, a replacement, supply restored.
    assert_eq!(supply(&game, 1, "climate")["status"], "supplied");
    run_until(&mut game, 30, |game| failed(game, u64::from(CLIMATE)));
    assert_eq!(alerts(&game, "climate_shortage").len(), 1);
    assert!(!failed(&game, u64::from(WATER)));
    ok(game.replace_utility(CLIMATE));
    assert_eq!(supply(&game, 1, "climate")["status"], "supplied");
}
