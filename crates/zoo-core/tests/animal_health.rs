//! #109: animal health, veterinary care, quarantine and recoverable illness.
//!
//! A care failure (a keeper whose schedule leaves out cleaning) builds a diagnosable
//! condition with a warning first. Vets, not keepers, treat it: physically, with a medicine
//! kit from the depot, through the habitat's service gate. Missing vets, medicine, routes or
//! quarantine room each block with their own reason. Quarantine holds an ill animal apart
//! from its habitat and returns it home once recovered. Illness never kills, and recovery
//! always follows treatment and good care.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
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

fn animal(game: &ZooGame, id: u64) -> Value {
    view(game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|animal| animal["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}

fn health(game: &ZooGame, id: u64) -> Value {
    animal(game, id)["health"].clone()
}

fn status(game: &ZooGame, id: u64) -> String {
    health(game, id)["status"].as_str().unwrap().to_owned()
}

fn depot(game: &ZooGame) -> Value {
    view(game)["animal_care_depot"].clone()
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

fn alerts(game: &ZooGame, code: &str) -> Vec<Value> {
    view(game)["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|alert| alert["code"] == code)
        .cloned()
        .collect()
}

fn vets(game: &ZooGame) -> Vec<Value> {
    depot(game)["keepers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|worker| worker["role"] == "vet")
        .cloned()
        .collect()
}

/// Open treatment jobs as (patient, habitat), across every habitat.
fn treatments(game: &ZooGame) -> Vec<(u64, u64)> {
    view(game)["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|habitat| habitat["care_jobs"].as_array().unwrap().clone())
        .filter(|job| job["category"] == "treatment")
        .filter(|job| matches!(job["state"].as_str(), Some("due" | "in_progress")))
        .map(|job| {
            (
                job["animal_id"].as_u64().unwrap(),
                job["habitat_id"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// Medicine kits on the shelf and in vets' hands.
fn kits_in_park(game: &ZooGame) -> u64 {
    let backlog = &depot(game)["care_backlog"];
    backlog["medicine_on_shelf"].as_u64().unwrap() + backlog["medicine_carried"].as_u64().unwrap()
}

/// Ticks one minute at a time until `done`, at most `limit` minutes; returns the minutes run.
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

/// Treatments given so far, by patient: each unwell → recovering transition seen.
#[derive(Default)]
struct Ledger {
    statuses: BTreeMap<u64, String>,
    treated: BTreeMap<u64, u32>,
}

impl Ledger {
    fn observe(&mut self, game: &ZooGame) {
        for animal in view(game)["animals"].as_array().unwrap() {
            let id = animal["id"].as_u64().unwrap();
            let now = animal["health"]["status"].as_str().unwrap().to_owned();
            if self.statuses.get(&id).map(String::as_str) == Some("unwell") && now == "recovering" {
                *self.treated.entry(id).or_default() += 1;
            }
            self.statuses.insert(id, now);
        }
    }

    fn total(&self) -> u64 {
        self.treated.values().map(|&count| u64::from(count)).sum()
    }
}

/// Runs minute by minute, checking that medicine is conserved and that every animal lives
/// in exactly one habitat with at most one open treatment; returns when `done` holds.
fn run_checked(
    game: &mut ZooGame,
    ledger: &mut Ledger,
    bought: u64,
    limit: u32,
    done: impl Fn(&ZooGame) -> bool,
) {
    for _ in 0..limit {
        ledger.observe(game);
        assert_eq!(
            kits_in_park(game) + ledger.total(),
            bought,
            "every kit is on the shelf, in a vet's hands or used by exactly one treatment"
        );
        let snapshot = view(game);
        let animals = snapshot["animals"].as_array().unwrap();
        let ids: BTreeSet<u64> = animals.iter().map(|a| a["id"].as_u64().unwrap()).collect();
        assert_eq!(ids.len(), animals.len(), "an animal is listed twice");
        let mut patients = BTreeSet::new();
        for (patient, _) in treatments(game) {
            assert!(
                patients.insert(patient),
                "two open treatments for {patient}"
            );
        }
        if done(game) {
            return;
        }
        game.tick(1);
    }
    panic!("the condition never held within {limit} minutes");
}

/// Two capybaras (#1 and #2) in habitat #1 whose only keeper's schedule leaves out cleaning,
/// beside an empty 3×3 habitat #2; one staffed zone and feed. No vet and no medicine yet.
fn neglected(capybaras: u32) -> ZooGame {
    let mut game = ZooGame::new();
    for x in 2..=14 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 7, 10));
    ok(game.place_habitat_rect(9, 8, 11, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.set_keeper_schedule(1, "feeding,inspection,enrichment".to_owned(), 0, 1440));
    for _ in 0..3 {
        ok(game.buy_animal_feed());
    }
    ok(game.adopt_group(1, "capybara".to_owned(), capybaras));
    game
}

/// The neglected park once its capybaras have fallen ill.
fn ill(capybaras: u32) -> ZooGame {
    let mut game = neglected(capybaras);
    run_until(&mut game, 2000, |game| status(game, 1) == "unwell");
    game
}

/// Demolishes a path tile as soon as no guest stands on it.
fn demolish(game: &mut ZooGame, x: u32, y: u32) {
    for _ in 0..120 {
        if parse(&game.bulldoze(x, y))["ok"] == true {
            return;
        }
        game.tick(1);
    }
    panic!("({x}, {y}) never cleared of guests");
}

fn restore_cleaning(game: &mut ZooGame) {
    ok(game.set_keeper_schedule(
        1,
        "feeding,cleaning,inspection,enrichment".to_owned(),
        0,
        1440,
    ));
}

/// The service gate's outside tile of a habitat: the path tile keepers and vets enter from.
fn gate_access(game: &ZooGame, habitat_id: u64) -> (u32, u32) {
    let access = &habitat(game, habitat_id)["service_gate"]["access"];
    (
        access["x"].as_u64().unwrap() as u32,
        access["y"].as_u64().unwrap() as u32,
    )
}

#[test]
fn a_care_failure_warns_then_causes_a_diagnosable_condition_that_treatment_and_care_cure() {
    let mut game = neglected(2);

    // Warning first: the cause and the fix, before anything is wrong.
    run_until(&mut game, 2000, |game| status(game, 1) == "at_risk");
    let warning = health(&game, 1);
    assert_eq!(warning["condition"], "sniffles");
    assert!(
        warning["summary"]
            .as_str()
            .unwrap()
            .contains("habitat #1 is dirty"),
        "{warning}"
    );
    assert!(warning["risk_percent"].as_u64().unwrap() >= 50);
    let notice = alerts(&game, "animal_at_risk");
    assert_eq!(notice.len(), 1, "one notice for the habitat");
    assert!(
        notice[0]["message"]
            .as_str()
            .unwrap()
            .contains("may come down with the sniffles"),
        "{notice:?}"
    );

    // The condition: diagnosable, gentle, waiting for a vet nobody has hired.
    run_until(&mut game, 600, |game| status(game, 1) == "unwell");
    for id in [1, 2] {
        let unwell = health(&game, id);
        assert_eq!(unwell["status"], "unwell");
        assert_eq!(unwell["condition"], "sniffles");
        assert!(
            unwell["summary"]
                .as_str()
                .unwrap()
                .contains("sneezing and resting"),
            "{unwell}"
        );
        assert_eq!(animal(&game, id)["activity"], "resting");
    }
    let parts = habitat(&game, 1)["welfare_parts"].clone();
    let part = parts
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["key"] == "health")
        .expect("an unwell group shows a health part");
    assert_eq!(part["score"], 0);
    assert_eq!(part["detail"], "2 of 2 unwell or recovering");

    // Untreated illness is never fatal: a day later both are still here, still unwell.
    game.tick(1440);
    assert_eq!(status(&game, 1), "unwell");
    assert_eq!(status(&game, 2), "unwell");

    // A vet and medicine: each is treated in place, physically.
    let cash = view(&game)["cash_cents"].as_i64().unwrap();
    ok(game.hire_vet());
    ok(game.buy_medicine());
    let finance = &view(&game)["finance"]["current_day"]["breakdown"];
    assert_eq!(finance["vet_hiring_expense_cents"], 30_000);
    assert_eq!(finance["medicine_expense_cents"], 4_000);
    assert_eq!(
        view(&game)["cash_cents"].as_i64().unwrap(),
        cash - 34_000,
        "hiring and medicine flow into the ledger"
    );
    let mut ledger = Ledger::default();
    let mut carried = false;
    for _ in 0..400 {
        ledger.observe(&game);
        carried |= vets(&game)[0]["carrying_medicine"] == true;
        if status(&game, 1) == "recovering" && status(&game, 2) == "recovering" {
            break;
        }
        game.tick(1);
    }
    assert!(carried, "the vet carried a kit from the depot");
    assert_eq!(ledger.treated, BTreeMap::from([(1, 1), (2, 1)]));
    assert_eq!(depot(&game)["medicine_kits"], 3);

    // Recovery pauses while the habitat stays dirty; it never regresses.
    game.tick(600);
    let paused = health(&game, 1);
    assert_eq!(paused["status"], "recovering");
    assert_eq!(paused["recovery_percent"], 0);
    assert!(
        paused["recovery_paused"]
            .as_str()
            .unwrap()
            .contains("dirty"),
        "{paused}"
    );

    // Improved care: the keeper cleans again and both recover.
    restore_cleaning(&mut game);
    run_until(&mut game, 1440, |game| {
        status(game, 1) == "healthy" && status(game, 2) == "healthy"
    });
    assert!(alerts(&game, "animal_unwell").is_empty());
    assert!(
        habitat(&game, 1)["welfare_parts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|part| part["key"] != "health"),
        "a healthy group has no health part"
    );
    // Vets cost wages every hour from now on.
    game.tick(120);
    assert!(
        view(&game)["finance"]["current_day"]["breakdown"]["vet_wages_expense_cents"]
            .as_i64()
            .unwrap()
            > 0
    );
}

#[test]
fn missing_vets_medicine_and_routes_block_treatment_without_remote_cures() {
    let mut game = ill(2);
    let reason = |game: &ZooGame| {
        health(game, 1)["treatment"]["waiting_reason"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    // No vet.
    assert_eq!(health(&game, 1)["treatment"]["phase"], "blocked");
    assert!(reason(&game).contains("No vet is on staff"));
    let alert = &alerts(&game, "animal_unwell")[0];
    assert_eq!(alert["action"], "hire_vet");
    assert_eq!(alert["subject"], json!({ "kind": "animal", "id": 1 }));

    // A vet, but no medicine.
    ok(game.hire_vet());
    game.tick(30);
    assert_eq!(status(&game, 1), "unwell");
    assert!(reason(&game).contains("out of unreserved medicine kits"));
    assert_eq!(alerts(&game, "animal_unwell")[0]["action"], "buy_medicine");
    assert!(
        vets(&game)[0]["status"]
            .as_str()
            .unwrap()
            .contains("anywhere in the park")
    );

    // Medicine, but the gate cannot be reached: nobody is treated remotely.
    let (x, y) = gate_access(&game, 1);
    demolish(&mut game, x, y);
    ok(game.buy_medicine());
    game.tick(600);
    assert_eq!(status(&game, 1), "unwell");
    assert_eq!(status(&game, 2), "unwell");
    assert!(
        reason(&game).contains("Vets cannot reach habitat #1"),
        "{}",
        reason(&game)
    );
    assert_eq!(alerts(&game, "animal_unwell")[0]["action"], "build_path");
    assert_eq!(depot(&game)["medicine_kits"], 5, "no kit left the shelf");

    // Rebuilt access: the vet walks in and treats both.
    ok(game.place_path(x, y));
    run_until(&mut game, 400, |game| {
        status(game, 1) == "recovering" && status(game, 2) == "recovering"
    });
    assert_eq!(depot(&game)["medicine_kits"], 3);
}

#[test]
fn quarantine_refuses_healthy_animals_and_full_or_unreachable_habitats() {
    let mut game = neglected(4);
    // Healthy animals stay out.
    let healthy = parse(&game.evaluate_quarantine(1));
    assert_eq!(healthy["ok"], false);
    assert!(healthy["message"].as_str().unwrap().contains("is healthy"));
    run_until(&mut game, 2000, |game| status(game, 4) == "unwell");

    // No quarantine habitat yet.
    let none = parse(&game.evaluate_quarantine(1));
    assert!(
        none["message"]
            .as_str()
            .unwrap()
            .starts_with("No habitat is set aside for quarantine"),
        "{none}"
    );
    rejected(game.quarantine_animal(1));

    // Only an empty habitat can be set aside, and then nobody is adopted into it.
    let stocked = rejected(game.set_habitat_quarantine(1, true));
    assert!(
        stocked["message"]
            .as_str()
            .unwrap()
            .contains("Move habitat #1's animals out")
    );
    ok(game.set_habitat_quarantine(2, true));
    assert_eq!(habitat(&game, 2)["quarantine"], true);
    let adoption = parse(&game.evaluate_adoption(2, "capybara".to_owned(), 1));
    assert_eq!(adoption["ok"], false);
    assert!(
        adoption["message"]
            .as_str()
            .unwrap()
            .contains("set aside for quarantine")
    );
    let transfer = parse(&game.evaluate_transfer(1, 2));
    assert_eq!(transfer["ok"], false);
    assert!(
        transfer["message"]
            .as_str()
            .unwrap()
            .contains("Move to quarantine")
    );

    // Unreachable: a cut path to its gate keeps patients out.
    let (x, y) = gate_access(&game, 2);
    demolish(&mut game, x, y);
    let cut = parse(&game.evaluate_quarantine(1));
    assert!(
        cut["message"]
            .as_str()
            .unwrap()
            .contains("Quarantine habitat #2 cannot be reached"),
        "{cut}"
    );
    ok(game.place_path(x, y));

    // A 3×3 quarantine has three places; the fourth patient finds it full.
    for id in 1..=3 {
        let moved = ok(game.quarantine_animal(id));
        assert!(
            moved["message"]
                .as_str()
                .unwrap()
                .contains("into quarantine habitat #2")
        );
    }
    let full = parse(&game.evaluate_quarantine(4));
    assert_eq!(full["ok"], false);
    assert_eq!(
        full["message"],
        "Quarantine habitat #2 is full (3 of 3 places)"
    );
    // A habitat with patients stays a quarantine.
    let busy = rejected(game.set_habitat_quarantine(2, false));
    assert!(
        busy["message"]
            .as_str()
            .unwrap()
            .contains("still cares for 3 patients")
    );
}

#[test]
fn an_animal_lives_in_one_place_and_is_treated_once_per_illness() {
    let mut game = ill(2);
    ok(game.set_habitat_quarantine(2, true));
    let moved = ok(game.quarantine_animal(1));
    assert_eq!(
        moved["message"],
        format!(
            "{} (#1) moved from habitat #1 into quarantine habitat #2",
            animal(&game, 1)["name"].as_str().unwrap()
        )
    );
    // Exactly one place: listed once, in quarantine, not at home.
    assert_eq!(animal(&game, 1)["habitat_id"], 2);
    assert_eq!(habitat(&game, 1)["animals"], 1);
    assert_eq!(habitat(&game, 2)["animals"], 1);
    let record = health(&game, 1);
    assert_eq!(record["in_quarantine"], true);
    assert_eq!(record["quarantine_home"], 1);
    assert!(
        rejected(game.quarantine_animal(1))["message"]
            .as_str()
            .unwrap()
            .contains("already in quarantine")
    );
    assert!(
        rejected(game.transfer_animal(1, 1))["message"]
            .as_str()
            .unwrap()
            .contains("stays in quarantine until it has recovered")
    );
    // The treatment followed the patient: one open job each, where each lives.
    let mut open = treatments(&game);
    open.sort_unstable();
    assert_eq!(open, vec![(1, 2), (2, 1)]);

    // Two vets for two patients, ten kits: each patient is treated exactly once.
    ok(game.hire_vet());
    ok(game.hire_vet());
    ok(game.buy_medicine());
    ok(game.buy_medicine());
    restore_cleaning(&mut game);
    let mut ledger = Ledger::default();
    run_checked(&mut game, &mut ledger, 10, 2000, |game| {
        status(game, 1) == "healthy"
            && status(game, 2) == "healthy"
            && animal(game, 1)["habitat_id"] == 1
    });
    assert_eq!(ledger.treated, BTreeMap::from([(1, 1), (2, 1)]));
    assert_eq!(depot(&game)["medicine_kits"], 8);
    // Recovered and released: home again, the quarantine empty and free to return to use.
    let released = health(&game, 1);
    assert_eq!(released["in_quarantine"], false);
    assert_eq!(released["quarantine_home"], Value::Null);
    assert_eq!(habitat(&game, 2)["animals"], 0);
    ok(game.set_habitat_quarantine(2, false));
}

#[test]
fn the_sniffles_spreads_only_within_a_habitat_and_quarantine_stops_it() {
    // Habitat #1's capybaras fall ill; habitat #2 gets a healthy pair, cleaned by the keeper.
    let mut game = ill(2);
    restore_cleaning(&mut game);
    for x in 3..=7 {
        ok(game.place_path(x, 6));
    }
    ok(game.place_habitat_rect(3, 2, 7, 5));
    ok(game.adopt_group(3, "capybara".to_owned(), 2));
    let (mates, patient) = ([3_u64, 4], 1_u32);
    for id in mates {
        assert_eq!(animal(&game, id)["habitat_id"], 3);
    }
    // An untreated patient moved in spreads the sniffles to its new mates, with a warning
    // that names it.
    ok(game.transfer_animal(1, 3));
    run_until(&mut game, 600, |game| status(game, 3) == "at_risk");
    let warning = health(&game, 3);
    assert!(
        warning["summary"]
            .as_str()
            .unwrap()
            .contains(&format!("(#{patient}) has the sniffles nearby")),
        "{warning}"
    );
    // Quarantine removes the source: the mates' risk eases instead of reaching onset.
    ok(game.set_habitat_quarantine(2, true));
    ok(game.quarantine_animal(patient));
    let risk = health(&game, 3)["risk_percent"].as_u64().unwrap();
    game.tick(240);
    let eased = health(&game, 3);
    assert_eq!(eased["status"], "healthy", "{eased}");
    assert!(eased["risk_percent"].as_u64().unwrap() < risk);
    // Inside quarantine nothing spreads: the other patient joins and nothing changes.
    ok(game.quarantine_animal(2));
    game.tick(60);
    for id in mates {
        assert_ne!(status(&game, id), "unwell");
    }
}

#[test]
fn interruptions_lost_access_reassignment_and_reloads_conserve_medicine_and_jobs() {
    let mut game = ill(2);
    // Guests can stand on paths; with the park closed and empty, demolition is immediate.
    ok(game.set_park_open(false));
    run_until(&mut game, 1440, |game| view(game)["guest_count"] == 0);
    ok(game.hire_vet());
    ok(game.buy_medicine());
    ok(game.set_habitat_quarantine(2, true));
    let vet_id = vets(&game)[0]["id"].as_u64().unwrap();
    // Vets are not keepers: no zone, no routine schedule.
    rejected(game.assign_keeper_zone(vet_id as u32, 1));
    rejected(game.set_keeper_schedule(vet_id as u32, "cleaning".to_owned(), 0, 1440));
    let mut ledger = Ledger::default();
    let carrying = |game: &ZooGame| vets(game)[0]["carrying_medicine"] == true;
    let patient_of_vet = |game: &ZooGame| {
        let job = vets(game)[0]["job_id"].as_u64().unwrap();
        view(game)["habitats"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|habitat| habitat["care_jobs"].as_array().unwrap().clone())
            .find(|candidate| candidate["id"] == job)
            .unwrap()["animal_id"]
            .as_u64()
            .unwrap()
    };

    // Lost access: the path to habitat #1's gate is demolished while the vet carries a kit
    // there; the vet hands the job back, walks the kit home, and the job waits blocked.
    run_checked(&mut game, &mut ledger, 5, 300, carrying);
    let (x, y) = gate_access(&game, 1);
    ok(game.bulldoze(x, y));
    run_checked(&mut game, &mut ledger, 5, 300, |game| {
        vets(game)[0]["activity"] == "at_depot"
    });
    assert_eq!(vets(&game)[0]["carrying_medicine"], false);
    assert_eq!(
        depot(&game)["medicine_kits"],
        5,
        "the kit is back on the shelf"
    );
    assert_eq!(health(&game, 1)["treatment"]["phase"], "blocked");
    assert_eq!(health(&game, 2)["treatment"]["phase"], "blocked");
    ok(game.place_path(x, y));

    // Reassignment: a patient moved to quarantine while the vet carries its kit hands the
    // job back; it follows the patient and the kit is never lost or used twice.
    run_checked(&mut game, &mut ledger, 5, 300, carrying);
    let patient = patient_of_vet(&game);
    ok(game.quarantine_animal(patient as u32));
    assert!(
        treatments(&game).contains(&(patient, 2)),
        "the treatment follows the patient into quarantine"
    );

    // A save and reload mid-treatment continues exactly like an uninterrupted park.
    run_checked(&mut game, &mut ledger, 5, 300, carrying);
    let saved = game.save_json();
    let mut reloaded = ZooGame::new();
    ok(reloaded.load_save(&saved));
    assert_eq!(reloaded.save_json(), saved);
    game.tick(45);
    reloaded.tick(45);
    assert_eq!(reloaded.save_json(), game.save_json());

    restore_cleaning(&mut game);
    run_checked(&mut game, &mut ledger, 5, 2000, |game| {
        status(game, 1) == "healthy" && status(game, 2) == "healthy"
    });
    assert_eq!(ledger.total(), 2, "one kit per illness");
    assert_eq!(kits_in_park(&game), 3);
}

#[test]
fn health_replays_deterministically_with_bounded_work_and_stable_ids() {
    let script = |game: &mut ZooGame| {
        run_until(game, 2000, |game| status(game, 1) == "unwell");
        ok(game.set_habitat_quarantine(2, true));
        ok(game.quarantine_animal(2));
        ok(game.hire_vet());
        ok(game.buy_medicine());
        game.tick(300);
        restore_cleaning(game);
        game.tick(900);
    };
    let mut first = neglected(2);
    let mut second = neglected(2);
    script(&mut first);
    script(&mut second);
    assert_eq!(first.save_json(), second.save_json());
    let replayed = ZooGame::replay(&first.content_json(), &first.diagnostics_json())
        .expect("the command log replays");
    assert_eq!(replayed.snapshot_json(), first.snapshot_json());

    // Stable identity throughout; bounded risk; at most one treatment per animal.
    let ids: Vec<u64> = view(&first)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|animal| animal["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![1, 2]);
    for id in [1, 2] {
        assert!(health(&first, id)["risk_percent"].as_u64().unwrap() <= 100);
    }
    assert!(treatments(&first).len() <= 2);
}

#[test]
fn content_cannot_schedule_treatment_as_keeper_work() {
    let mut catalog = parse(&ZooGame::new().content_json());
    catalog["species"][0]["care"]["schedule"][0]["category"] = json!("treatment");
    let error = ZooGame::from_content_json(&catalog.to_string())
        .err()
        .expect("treatment is not a scheduled task");
    assert!(error.contains("treatment is veterinary work"), "{error}");
}

const PROOF_PARK: &str = "tests/fixtures/animal-health-park.json";

/// The park the browser proof (apps/web/scripts/animal-health-proof.mjs) opens: capybaras #1
/// and #2 in habitat #1, whose only keeper does not clean, are close to falling ill with the
/// sniffles; habitat #2 is set aside for quarantine; no vet is hired and the depot has no
/// medicine. It must stay a current, loadable save; rewrite it with
/// `ZOO_WRITE_ANIMAL_HEALTH_PARK=1`.
#[test]
fn the_animal_health_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_ANIMAL_HEALTH_PARK").is_some() {
        let mut game = neglected(2);
        ok(game.set_habitat_quarantine(2, true));
        run_until(&mut game, 2000, |game| {
            health(game, 1)["risk_percent"].as_u64().unwrap() >= 90
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
    // The proof's journey: warning, illness, quarantine, a vet, recovery and homecoming.
    assert_eq!(status(&game, 1), "at_risk");
    assert!(vets(&game).is_empty());
    run_until(&mut game, 120, |game| status(game, 1) == "unwell");
    ok(game.quarantine_animal(1));
    ok(game.hire_vet());
    ok(game.buy_medicine());
    run_until(&mut game, 900, |game| {
        status(game, 1) == "healthy" && animal(game, 1)["habitat_id"] == 1
    });
}
