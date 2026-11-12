//! #104: keeper care is physical. Keepers collect feed at the depot, carry it through reachable
//! service gates to a care spot inside the habitat, and only there is care done and stock
//! consumed. Blocked routes and empty depots delay care with specific alerts, never remote
//! servicing; stock, jobs and money reconcile through interruptions, cancellations and reloads.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
}

fn view(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

fn keepers(view: &Value) -> Vec<Value> {
    view["animal_care_depot"]["keepers"]
        .as_array()
        .unwrap()
        .clone()
}

fn jobs(view: &Value) -> Vec<Value> {
    view["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|habitat| habitat["care_jobs"].as_array().unwrap().clone())
        .collect()
}

fn habitat(view: &Value, id: u64) -> Value {
    view["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|habitat| habitat["id"] == id)
        .unwrap()
        .clone()
}

fn alert(view: &Value, code: &str, habitat_id: u64) -> Option<Value> {
    view["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|alert| alert["code"] == code && alert["subject"]["id"] == habitat_id)
        .cloned()
}

fn tile(value: &Value) -> (u64, u64) {
    (value["x"].as_u64().unwrap(), value["y"].as_u64().unwrap())
}

/// Two capybara habitats beside one path, both in one work zone, with `keepers` keepers and
/// (optionally) one batch of feed at the depot.
fn park(keepers: u32, feed: bool) -> ZooGame {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    if feed {
        ok(game.buy_animal_feed());
    }
    for keeper_id in 1..=keepers {
        ok(game.hire_keeper());
        ok(game.assign_keeper_zone(keeper_id, 1));
    }
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.adopt(2, "capybara".to_owned()));
    game
}

/// The same park with its animals hungry (food 40), so the next feeding needs a crate.
fn hungry(game: ZooGame) -> ZooGame {
    let mut save = json(&game.save_json());
    for habitat in save["park"]["habitats"].as_array_mut().unwrap() {
        if !habitat["species"].is_null() {
            habitat["food"] = 40.into();
        }
    }
    let mut game = ZooGame::new();
    ok(game.load_save(&save.to_string()));
    game
}

#[test]
fn a_keeper_services_two_habitats_through_reachable_gates_with_real_stock() {
    let mut game = hungry(park(1, true));
    let start = view(&game);
    let gates: Vec<(u64, u64)> = [1, 2]
        .iter()
        .map(|&id| tile(&habitat(&start, id)["service_gate"]["access"]))
        .collect();
    let mut visited = BTreeSet::new();
    let mut fed_at = BTreeMap::new();
    let mut pickups = 0;
    let mut carrying = false;
    let mut consumed = BTreeSet::new();
    for _ in 0..300 {
        game.tick(1);
        let view = view(&game);
        let keeper = &keepers(&view)[0];
        visited.insert(tile(keeper));
        let now_carrying = keeper["carrying_feed"] == true;
        if now_carrying && !carrying {
            // Crates are only ever picked up on the depot tile.
            assert_eq!(tile(keeper), tile(&view["animal_care_depot"]), "{keeper}");
            pickups += 1;
        }
        carrying = now_carrying;
        let jobs = jobs(&view);
        if keeper["activity"] == "working" {
            let job = jobs
                .iter()
                .find(|job| job["id"] == keeper["job_id"])
                .unwrap();
            // Care happens on the job's spot inside the habitat, never at the gate or remotely.
            assert_eq!(tile(keeper), tile(&job["care_spot"]), "{job}");
            // Animals that are not hungry yet need no crate; hungry ones get the carried one.
            if job["category"] == "feeding" && job["supply"] != "none" {
                assert_eq!(job["supply"], "feed_carried");
                assert!(now_carrying, "feeding without a crate in hand: {keeper}");
                fed_at.insert(job["habitat_id"].as_u64().unwrap(), tile(keeper));
            }
        }
        for job in &jobs {
            if job["category"] == "feeding"
                && job["state"] == "completed"
                && job["supply"] == "feed_carried"
            {
                consumed.insert(job["id"].as_u64().unwrap());
            }
        }
        let shelf = view["animal_care_depot"]["feed_crates"].as_u64().unwrap();
        assert_eq!(
            shelf + u64::from(now_carrying) + consumed.len() as u64,
            10,
            "every bought crate is on the shelf, in hand or fed"
        );
    }
    assert_eq!(
        fed_at.keys().copied().collect::<Vec<_>>(),
        [1, 2],
        "{fed_at:?}"
    );
    for gate in &gates {
        assert!(visited.contains(gate), "never walked through gate {gate:?}");
    }
    assert!(pickups >= 2, "{pickups} crate pickups");
    assert!(consumed.len() >= 2);
    let view = view(&game);
    let keeper = &keepers(&view)[0];
    assert_eq!(keeper["deliveries_completed"], consumed.len() as u64);
}

/// The second habitat's gate opens onto the path east of a cut tile; habitat 1 stays served.
#[test]
fn a_disconnected_gate_delays_care_with_a_specific_alert_and_never_services_remotely() {
    let mut game = hungry(park(1, true));
    let access = tile(&habitat(&view(&game), 2)["service_gate"]["access"]);
    assert!(access.0 >= 7, "{access:?}");
    ok(game.bulldoze(6, 7));
    let start = view(&game);
    assert_eq!(habitat(&start, 2)["service_gate"]["reachable"], false);
    let tiles: BTreeSet<(u64, u64)> = habitat(&start, 2)["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(tile)
        .collect();

    let mut food = habitat(&start, 2)["food"].as_u64().unwrap();
    let mut late = None;
    let mut blocked = false;
    for _ in 0..240 {
        game.tick(1);
        let view = view(&game);
        let now = habitat(&view, 2)["food"].as_u64().unwrap();
        assert!(now <= food, "habitat 2 was fed remotely");
        food = now;
        for keeper in keepers(&view) {
            assert!(!tiles.contains(&tile(&keeper)), "{keeper}");
        }
        for job in habitat(&view, 2)["care_jobs"].as_array().unwrap() {
            assert_ne!(job["state"], "completed", "{job}");
            if job["state"] == "due" {
                assert_eq!(job["phase"], "blocked", "{job}");
                blocked = true;
            }
        }
        late = late.or_else(|| alert(&view, "care_late", 2));
        assert!(alert(&view, "care_late", 1).is_none());
        let unreachable = alert(&view, "gate_unreachable", 2).expect("gate alert");
        assert_eq!(tile(&unreachable["focus"]), access);
    }
    assert!(blocked);
    let late = late.expect("late care alert");
    assert!(
        late["message"]
            .as_str()
            .unwrap()
            .contains("Gate unreachable"),
        "{late}"
    );
    assert_eq!(late["action"], "build_path");
    let view = view(&game);
    assert!(alert(&view, "care_missed", 2).is_some());
    // Habitat 1's gate is still reachable, and it is still fed.
    assert!(
        habitat(&view, 1)["care_jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|job| job["category"] == "feeding" && job["state"] == "completed")
    );

    // Reconnecting the gate lets the next feeding through.
    ok(game.place_path(6, 7));
    let mut fed = false;
    for _ in 0..180 {
        game.tick(1);
        if habitat(&view_of(&game), 2)["food"] == 100 {
            fed = true;
            break;
        }
    }
    assert!(fed, "habitat 2 was never fed after reconnecting its gate");
}

fn view_of(game: &ZooGame) -> Value {
    view(game)
}

#[test]
fn an_empty_depot_delays_feeding_with_a_specific_alert_until_feed_arrives() {
    let mut game = hungry(park(1, false));
    let feeding = |view: &Value| {
        habitat(view, 1)["care_jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["category"] == "feeding" && job["state"] == "due")
            .cloned()
    };
    // The first feeding finds the animals fed; the next finds them hungry and the shelf bare.
    let mut minutes = 0;
    let view = loop {
        game.tick(1);
        minutes += 1;
        assert!(
            minutes < 360,
            "no feeding ever waited half an hour for stock"
        );
        let view = view(&game);
        if alert(&view, "care_late", 1).is_some() {
            break view;
        }
    };
    let job = feeding(&view).expect("a due feeding");
    assert_eq!(job["phase"], "blocked");
    assert!(
        job["waiting_reason"]
            .as_str()
            .unwrap()
            .contains("out of unreserved feed"),
        "{job}"
    );
    assert!(
        view["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alert| alert["code"] == "feed_depot_empty")
    );
    let late = alert(&view, "care_late", 1).unwrap();
    assert!(
        late["message"].as_str().unwrap().contains("feeding"),
        "{late}"
    );
    assert_eq!(late["action"], "buy_feed");
    let backlog = &view["animal_care_depot"]["care_backlog"];
    assert!(
        backlog["supply_shortages"].as_u64().unwrap() >= 1,
        "{backlog}"
    );
    assert!(backlog["oldest_wait_minutes"].as_u64().unwrap() >= 30);
    assert_eq!(backlog["feed_carried"], 0);
    assert!(habitat(&view, 1)["food"].as_u64().unwrap() < 90);

    ok(game.buy_animal_feed());
    let mut fed = BTreeSet::new();
    for _ in 0..240 {
        game.tick(1);
        for id in [1, 2] {
            if habitat(&view_of(&game), id)["food"] == 100 {
                fed.insert(id);
            }
        }
    }
    assert_eq!(fed.into_iter().collect::<Vec<_>>(), [1, 2]);
}

/// The crate ledger at one moment: on the shelf, in hand, and fed so far.
#[derive(Default)]
struct Crates {
    bought: u64,
    consumed: BTreeSet<u64>,
    completed: BTreeSet<u64>,
}

impl Crates {
    fn check(&mut self, game: &ZooGame) {
        let view = view(game);
        let keepers = keepers(&view);
        let carried = keepers
            .iter()
            .filter(|keeper| keeper["carrying_feed"] == true)
            .count() as u64;
        for job in jobs(&view) {
            let id = job["id"].as_u64().unwrap();
            if job["state"] == "completed" {
                self.completed.insert(id);
                if job["category"] == "feeding" && job["supply"] == "feed_carried" {
                    self.consumed.insert(id);
                }
            } else {
                assert!(!self.completed.contains(&id), "job {id} reopened: {job}");
            }
            if job["state"] == "in_progress" {
                // One reservation per job: its crew points back at it, and only it.
                for keeper_id in job["keeper_ids"].as_array().unwrap() {
                    let keeper = keepers.iter().find(|k| &k["id"] == keeper_id).unwrap();
                    assert_eq!(keeper["job_id"], job["id"], "{keeper}");
                }
            }
        }
        let shelf = view["animal_care_depot"]["feed_crates"].as_u64().unwrap();
        let backlog = &view["animal_care_depot"]["care_backlog"];
        assert_eq!(backlog["feed_carried"], carried);
        assert!(backlog["feed_reserved"].as_u64().unwrap() <= shelf);
        assert_eq!(
            shelf + carried + self.consumed.len() as u64,
            self.bought,
            "feed is conserved"
        );
    }
}

/// Two zones split the two habitats; three keepers. Midway a carrying keeper is moved to the
/// other zone, the second habitat's path is cut past its grace hour and restored, and finally
/// the first habitat is demolished. Returns the final save.
fn eventful_day(reload_at: Option<u32>) -> String {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.buy_animal_feed());
    ok(game.create_work_zone(0, 0, 6, 13));
    ok(game.create_work_zone(7, 0, 19, 13));
    for (keeper_id, zone_id) in [(1, 1), (2, 1), (3, 2)] {
        ok(game.hire_keeper());
        ok(game.assign_keeper_zone(keeper_id, zone_id));
    }
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.adopt(2, "capybara".to_owned()));
    let mut crates = Crates {
        bought: 10,
        ..Crates::default()
    };
    let mut interrupted = false;
    let mut cut = false;
    for minute in 0..600 {
        if Some(minute) == reload_at {
            let save = game.save_json();
            game = ZooGame::new();
            ok(game.load_save(&save));
        }
        game.tick(1);
        crates.check(&game);
        let view = view(&game);
        if !interrupted {
            // Interruption after pickup: the carrier leaves for the other zone.
            if let Some(keeper) = keepers(&view).into_iter().find(|keeper| {
                keeper["carrying_feed"] == true
                    && keeper["zone_id"] == 1
                    && tile(keeper) != tile(&view["animal_care_depot"])
            }) {
                let job_id = keeper["job_id"].clone();
                ok(game.assign_keeper_zone(keeper["id"].as_u64().unwrap() as u32, 2));
                game.tick(1);
                crates.check(&game);
                let view = view_of(&game);
                let job = jobs(&view)
                    .into_iter()
                    .find(|job| job["id"] == job_id)
                    .unwrap();
                assert!(
                    job["state"] == "due" || job["keeper_ids"] != serde_json::json!([keeper["id"]]),
                    "{job}"
                );
                let moved = keepers(&view)
                    .into_iter()
                    .find(|k| k["id"] == keeper["id"])
                    .unwrap();
                assert_eq!(moved["activity"], "to_depot");
                assert_eq!(moved["carrying_feed"], true, "the crate walks back");
                interrupted = true;
            }
        }
        // Cut the second habitat off (once no guest stands on the tile) past a grace hour.
        if (200..330).contains(&minute) && !cut {
            cut = json(&game.bulldoze(6, 7))["ok"] == true;
        }
        if minute == 330 {
            assert!(cut, "the path was never cut");
            ok(game.place_path(6, 7));
        }
        if minute == 480 {
            ok(game.buy_animal_feed());
            crates.bought += 10;
        }
    }
    assert!(interrupted, "no keeper ever carried feed in zone 1");
    // Demolishing a habitat cancels its jobs and puts any crate for it back on the shelf.
    ok(game.bulldoze(4, 9));
    crates.check(&game);
    for _ in 0..60 {
        game.tick(1);
        crates.check(&game);
    }
    assert!(
        crates.consumed.len() >= 4,
        "{} feedings",
        crates.consumed.len()
    );
    game.save_json()
}

#[test]
fn feed_and_jobs_reconcile_through_interruptions_cancellations_and_reloads() {
    let uninterrupted = eventful_day(None);
    for reload_at in [97, 211, 345] {
        assert_eq!(
            eventful_day(Some(reload_at)),
            uninterrupted,
            "reload at {reload_at}"
        );
    }
}

/// One keeper, three habitats: more care than one keeper can do. The third habitat's feeding
/// is made top priority; a second zone's idle keeper is never borrowed.
fn overloaded() -> ZooGame {
    let mut game = ZooGame::new();
    for x in 5..=15 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    ok(game.place_habitat_rect(11, 8, 13, 10));
    ok(game.create_work_zone(0, 0, 16, 13));
    ok(game.create_work_zone(17, 0, 19, 13));
    ok(game.buy_animal_feed());
    ok(game.buy_animal_feed());
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(2, 2));
    for id in 1..=3 {
        ok(game.adopt(id, "capybara".to_owned()));
    }
    ok(game.set_care_task(3, "feeding".to_owned(), 420, 120, 5));
    game
}

#[test]
fn zones_and_priorities_are_deterministic_and_starvation_bounded() {
    let mut game = overloaded();
    let mut twin = overloaded();
    let mut outcomes: BTreeMap<(u64, String, String), BTreeSet<u64>> = BTreeMap::new();
    for _ in 0..720 {
        game.tick(1);
        twin.tick(1);
        let view = view(&game);
        for job in jobs(&view) {
            let waited = job["waited_minutes"].as_u64().unwrap();
            match job["state"].as_str().unwrap() {
                // Starvation bound: no job waits for a crew past its grace hour.
                "due" => assert!(waited < 60, "{job}"),
                "in_progress" => assert_eq!(job["keeper_ids"], serde_json::json!([1]), "{job}"),
                state => {
                    outcomes
                        .entry((
                            job["habitat_id"].as_u64().unwrap(),
                            job["category"].as_str().unwrap().to_owned(),
                            state.to_owned(),
                        ))
                        .or_default()
                        .insert(job["id"].as_u64().unwrap());
                }
            }
        }
        // The other zone's keeper is never borrowed.
        let idle = &keepers(&view)[1];
        assert!(idle["job_id"].is_null(), "{idle}");
    }
    assert_eq!(game.snapshot_json(), twin.snapshot_json());
    let count = |habitat: u64, category: &str, state: &str| {
        outcomes
            .get(&(habitat, category.to_owned(), state.to_owned()))
            .map_or(0, BTreeSet::len)
    };
    // The top-priority feeding is never missed; lower-priority work is, and it shows.
    assert!(count(3, "feeding", "completed") >= 4);
    assert_eq!(count(3, "feeding", "missed"), 0);
    let missed: usize = [1, 2]
        .iter()
        .flat_map(|&habitat| {
            ["feeding", "cleaning", "inspection"].map(|category| count(habitat, category, "missed"))
        })
        .sum();
    assert!(
        missed > 0,
        "one keeper kept up with three habitats: {outcomes:?}"
    );
    let view = view(&game);
    assert!(
        view["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alert| alert["code"] == "care_missed")
    );
    assert!(
        view["animal_care_depot"]["care_backlog"]["missed_recent"]
            .as_u64()
            .unwrap()
            > 0
    );

    // Staffing correction: the idle keeper joins zone 1 and a third is hired there. Over an
    // equal stretch, far less care is missed.
    let missed_before: usize = outcomes
        .iter()
        .filter(|((_, _, state), _)| state == "missed")
        .map(|(_, ids)| ids.len())
        .sum();
    let resolved: BTreeSet<u64> = outcomes.values().flatten().copied().collect();
    ok(game.assign_keeper_zone(2, 1));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(3, 1));
    let mut missed_after = BTreeSet::new();
    for _ in 0..720 {
        game.tick(1);
        for job in jobs(&view_of(&game)) {
            let id = job["id"].as_u64().unwrap();
            if job["state"] == "missed" && !resolved.contains(&id) {
                missed_after.insert(id);
            }
        }
    }
    assert!(
        missed_after.len() * 2 < missed_before,
        "{} missed after the correction vs {missed_before} before",
        missed_after.len()
    );

    let replayed = ZooGame::replay(&twin.content_json(), &twin.diagnostics_json()).unwrap();
    assert_eq!(replayed.snapshot_json(), twin.snapshot_json());
}

fn ledger(game: &ZooGame) -> Value {
    view(game)["finance"]["current_day"]["breakdown"].clone()
}

/// A full day of care with one interrupted cleaning: feed, cleaning and wages are each booked
/// exactly once, and a reload books nothing again.
fn costed_day(reload_at: Option<u32>) -> (Value, BTreeSet<u64>, u64) {
    let mut game = park(2, true);
    // A spare zone in the corner to move a keeper to.
    ok(game.set_work_zone_bounds(1, 0, 0, 18, 13));
    ok(game.create_work_zone(19, 0, 19, 13));
    let mut cleanings = BTreeSet::new();
    let mut interrupted = None;
    // The park opens at 09:00; stay within the day so one ledger holds everything.
    for minute in 0..840 {
        if Some(minute) == reload_at {
            let save = game.save_json();
            game = ZooGame::new();
            ok(game.load_save(&save));
        }
        game.tick(1);
        let view = view(&game);
        for job in jobs(&view) {
            if job["category"] == "cleaning"
                && job["state"] == "completed"
                && job["supply"] == "cleaning"
            {
                cleanings.insert(job["id"].as_u64().unwrap());
            }
            if interrupted.is_none()
                && job["category"] == "cleaning"
                && job["state"] == "in_progress"
                && job["supply"] == "cleaning"
            {
                // Cancel the cleaning crew mid-job: the job is handed back, nothing is paid.
                let worker = job["keeper_ids"][0].as_u64().unwrap() as u32;
                ok(game.assign_keeper_zone(worker, 2));
                game.tick(1);
                ok(game.assign_keeper_zone(worker, 1));
                interrupted = Some(job["id"].as_u64().unwrap());
            }
        }
    }
    assert!(interrupted.is_some(), "no cleaning was ever dispatched");
    (ledger(&game), cleanings, interrupted.unwrap())
}

#[test]
fn wages_supplies_and_service_costs_reconcile_to_finance_exactly_once() {
    let content = json(&ZooGame::new().content_json());
    let cleaning = content["balance"]["habitat_care"]["cleaning_cents"]
        .as_i64()
        .unwrap();
    let wage = content["balance"]["staff"]["keeper"]["hourly_wage_cents"]
        .as_i64()
        .unwrap();
    let batch = content["balance"]["feed_batch_cents"].as_i64().unwrap();

    let (ledger, cleanings, _) = costed_day(None);
    assert!(!cleanings.is_empty());
    assert_eq!(
        ledger["habitat_care_expense_cents"].as_i64().unwrap(),
        cleaning * cleanings.len() as i64,
        "one cleaning charge per completed cleaning, none for the interrupted one"
    );
    assert_eq!(ledger["animal_feed_expense_cents"].as_i64().unwrap(), batch);
    let wages = ledger["keeper_wages_expense_cents"].as_i64().unwrap();
    assert_eq!(wages % (2 * wage), 0, "{wages}");
    assert_eq!(
        wages / (2 * wage),
        14,
        "two keepers paid for each of 14 hours"
    );

    for reload_at in [133, 401] {
        assert_eq!(
            costed_day(Some(reload_at)).0,
            ledger,
            "reload at {reload_at}"
        );
    }
}

const PROOF_PARK: &str = "tests/fixtures/keeper-care-park.json";

/// The browser proof's park (`apps/web/scripts/keeper-care-proof.mjs`): three hungry capybara
/// habitats in zone #1 with one keeper, while keepers #2 and #3 sit in an empty corner zone.
/// The proof watches care fall behind, moves both keepers into zone #1 from its inspector and
/// measures the recovery. It must stay a current, loadable save; rewrite it with
/// `ZOO_WRITE_KEEPER_CARE_PARK=1`.
#[test]
fn the_keeper_care_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_KEEPER_CARE_PARK").is_some() {
        let mut game = overloaded();
        ok(game.hire_keeper());
        ok(game.assign_keeper_zone(3, 2));
        game.tick(30);
        let game = hungry(game);
        let value = json(&game.save_json());
        std::fs::write(
            PROOF_PARK,
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
    let fixture = std::fs::read_to_string(PROOF_PARK).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    assert_eq!(json(&game.save_json()), json(&fixture));
    let view = view(&game);
    let zones: Vec<Value> = keepers(&view)
        .iter()
        .map(|k| k["zone_id"].clone())
        .collect();
    assert_eq!(zones, [1, 2, 2]);
    assert_eq!(view["habitats"].as_array().unwrap().len(), 3);
    game.tick(240);
    assert_eq!(game.session_status(), "running");
    assert!(
        view_of(&game)["animal_care_depot"]["care_backlog"]["missed_recent"].as_u64() > Some(0)
    );
}
