//! Education signs, keeper talks, guest learning, donations, conservation milestones and
//! grants (#116), through the public game seam: an attended talk or a readable sign teaches
//! while a decorative or unreachable sign does not; absent staff, full places, interruptions
//! and cancellation release reservations; donations and grants reconcile to the ledger and
//! never repeat; milestones name real eligible animals and resist adopt→release loops; save and
//! load keep schedules, eligibility and awards; the browser proof's park stays a current save.
use serde_json::Value;
use std::collections::BTreeMap;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) -> Value {
    let result = json(&result);
    assert_eq!(result["ok"], true, "{result}");
    result
}

fn refused(result: String) -> String {
    let result = json(&result);
    assert_eq!(result["ok"], false, "{result}");
    result["message"].as_str().unwrap().to_owned()
}

fn view(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

fn education(game: &ZooGame) -> Value {
    view(game)["education"].clone()
}

fn cents(value: &Value) -> i64 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("{value} is not whole cents"))
}

fn count(value: &Value) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("{value} is not a count"))
}

/// Ticks five minutes at a time until `done`, at most `limit` minutes.
fn run_until(game: &mut ZooGame, limit: u32, done: impl Fn(&ZooGame) -> bool) {
    for _ in 0..limit.div_ceil(5) {
        if done(game) {
            return;
        }
        game.tick(5);
    }
    assert!(
        done(game),
        "the condition never held within {limit} minutes"
    );
}

/// The lived-in starter park: capybaras in habitat #1 (4,5)–(8,9) and zebras in habitat #2
/// (10,11)–(14,15) on one public path along row 10 from the entrance, one keeper for both.
fn living() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.start_scenario("living-starter"));
    game
}

/// The starter park with a second keeper in its zone, so one is free for a talk while the other
/// cares for the animals.
fn staffed() -> ZooGame {
    let mut game = living();
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(2, 1));
    game
}

/// Runs `hours` while the player looks after the animals: water topped up every three hours
/// and feed bought twice a day.
fn care_for(game: &mut ZooGame, hours: u32) {
    for hour in 0..hours {
        if hour % 3 == 0 {
            for habitat in view(game)["habitats"].as_array().unwrap() {
                game.refill_water(count(&habitat["id"]) as u32);
            }
        }
        if hour % 12 == 0 {
            game.buy_animal_feed();
        }
        game.tick(60);
    }
}

/// The next quarter hour at least `lead` minutes away, as a minute of the day.
fn soon(game: &ZooGame, lead: u32) -> u32 {
    let minute = count(&view(game)["minute_of_day"]) as u32 + lead;
    (minute.div_ceil(15) * 15) % 1440
}

fn talk(game: &ZooGame, habitat_id: u64) -> Value {
    education(game)["talks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|talk| talk["habitat_id"] == habitat_id)
        .cloned()
        .unwrap_or(Value::Null)
}

fn guests(game: &ZooGame) -> Vec<Value> {
    view(game)["guests"].as_array().unwrap().clone()
}

/// Every guest's lessons, by guest ID.
fn lessons(game: &ZooGame) -> BTreeMap<u64, Vec<Value>> {
    guests(game)
        .into_iter()
        .map(|guest| {
            (
                count(&guest["id"]),
                guest["lessons"].as_array().unwrap().clone(),
            )
        })
        .collect()
}

fn sign(game: &ZooGame, x: u64, y: u64) -> Value {
    education(game)["signs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sign| sign["x"] == x && sign["y"] == y)
        .cloned()
        .unwrap()
}

/// Every income line of a ledger, so donations and grants count as income.
fn income(breakdown: &Value) -> i64 {
    breakdown
        .as_object()
        .unwrap()
        .iter()
        .filter(|(field, _)| field.ends_with("_income_cents"))
        .map(|(_, value)| cents(value))
        .sum()
}

/// Today's and every kept day's ledger explain their cash, and donations and grants in the
/// ledger equal what the education report says was given and paid.
fn assert_ledger_reconciles(game: &ZooGame) {
    let view = view(game);
    let finance = &view["finance"];
    let mut days: Vec<&Value> = finance["history"].as_array().unwrap().iter().collect();
    days.push(&finance["current_day"]);
    let (mut donations, mut grants) = (0, 0);
    for day in &days {
        let breakdown = &day["breakdown"];
        assert_eq!(
            cents(&day["income_cents"]),
            income(breakdown),
            "day {}",
            day["day"]
        );
        donations += cents(&breakdown["donation_income_cents"]);
        grants += cents(&breakdown["grant_income_cents"]);
        let expenses: i64 = breakdown
            .as_object()
            .unwrap()
            .iter()
            .filter(|(field, _)| field.ends_with("_expense_cents"))
            .map(|(_, value)| cents(value))
            .sum();
        let flow = income(breakdown) - expenses + cents(&breakdown["loan_proceeds_cents"])
            - cents(&breakdown["loan_repayment_cents"]);
        assert_eq!(
            cents(&day["opening_cash_cents"]) + flow,
            cents(&day["closing_cash_cents"]),
            "day {} does not reconcile",
            day["day"]
        );
    }
    assert_eq!(
        cents(&finance["current_day"]["closing_cash_cents"]),
        cents(&view["cash_cents"])
    );
    let total = &view["education"]["learning"]["total"];
    assert_eq!(donations, cents(&total["donations_cents"]));
    assert_eq!(grants, cents(&total["grants_cents"]));
    let attributed: i64 = view["education"]["donations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| cents(&source["cents"]))
        .sum();
    assert_eq!(attributed, cents(&total["donations_cents"]));
}

/// A sign at (3,11) beside the entrance path, two tiles from the capybaras; a decorative sign
/// at (1,11), three tiles from any habitat; and an unreachable sign at (5,4) on the capybaras'
/// north fence beside a public path (4..8, 3) that joins nothing.
fn signed() -> ZooGame {
    let mut game = staffed();
    ok(game.place_concession(3, 11, "sign".to_owned()));
    ok(game.place_concession(1, 11, "sign".to_owned()));
    ok(game.build_path_stroke(vec![4, 3, 8, 3], 1, "public".to_owned()));
    ok(game.place_concession(5, 4, "sign".to_owned()));
    game
}

#[test]
fn an_attended_talk_or_a_readable_sign_teaches_but_an_inaccessible_decorative_sign_does_not() {
    let mut game = signed();
    let reader = sign(&game, 3, 11);
    assert_eq!(reader["status"], "teaching", "{reader}");
    assert_eq!(reader["habitat_id"], 1);
    assert_eq!(reader["facts"][0]["species"], "capybara");
    assert!(
        reader["facts"][0]["source_url"]
            .as_str()
            .unwrap()
            .starts_with("https://"),
        "facts carry their source"
    );
    assert_eq!(sign(&game, 1, 11)["status"], "decorative");
    assert_eq!(sign(&game, 5, 4)["status"], "unreachable");

    // Guests read the sign on their way in: capybara, from habitat #1, at sign strength only.
    run_until(&mut game, 240, |game| {
        count(&education(game)["learning"]["total"]["guests_educated"]) >= 5
    });
    for lessons in lessons(&game).values() {
        for lesson in lessons {
            assert_eq!(lesson["species"], "capybara", "{lesson}");
            assert_eq!(lesson["habitat_id"], 1);
            assert_eq!(lesson["source"], "sign");
            assert_eq!(lesson["points"], 10);
        }
    }
    let learned: Vec<Value> = guests(&game)
        .into_iter()
        .filter(|guest| count(&guest["learning"]) > 0)
        .collect();
    assert!(!learned.is_empty());

    // The decorative and unreachable signs never taught anyone: every lesson above is the
    // capybara sign's, and a park with only those two signs teaches nobody.
    let mut decorative = living();
    ok(decorative.place_concession(1, 11, "sign".to_owned()));
    ok(decorative.build_path_stroke(vec![4, 3, 8, 3], 1, "public".to_owned()));
    ok(decorative.place_concession(5, 4, "sign".to_owned()));
    decorative.tick(240);
    assert_eq!(
        education(&decorative)["learning"]["total"]["guests_educated"],
        0
    );
    assert!(lessons(&decorative).values().all(Vec::is_empty));

    // A keeper talk at the capybaras: guests watching when it starts listen to the end and
    // learn more than a sign teaches, with a happier, better-valued visit.
    let start = soon(&game, 30);
    ok(game.schedule_talk(1, start, 20, 4));
    run_until(&mut game, 3 * 1440, |game| {
        count(&education(game)["learning"]["total"]["talks_given"]) >= 1
    });
    let outcome = talk(&game, 1)["last"].clone();
    assert_eq!(outcome["given"], true, "{outcome}");
    let attendees = count(&outcome["attendees"]);
    assert!((1..=4).contains(&attendees), "{outcome}");
    let taught: Vec<Value> = guests(&game)
        .into_iter()
        .filter(|guest| {
            guest["lessons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|lesson| lesson["source"] == "talk")
        })
        .collect();
    assert!(!taught.is_empty(), "attendees learned at the talk");
    for guest in &taught {
        let lesson = &guest["lessons"][0];
        assert_eq!(lesson["points"], 25, "{guest}");
        assert_eq!(lesson["species"], "capybara");
    }
    assert_eq!(
        count(&education(&game)["learning"]["total"]["attendees"]),
        attendees
    );
}

#[test]
fn absent_staff_full_places_interruptions_and_cancellation_release_reservations() {
    // Absent staff: the only keeper works another zone, so no one gives the talk. Viewers
    // reserve their places, then the talk is canceled and every place is released.
    let mut absent = living();
    ok(absent.create_work_zone(20, 0, 27, 3));
    ok(absent.assign_keeper_zone(1, 2));
    absent.tick(120);
    let start = soon(&absent, 15);
    ok(absent.schedule_talk(1, start, 20, 6));
    run_until(&mut absent, 120, |game| {
        talk(game, 1)["status"] == "waiting_for_keeper"
    });
    assert!(
        talk(&absent, 1)["detail"]
            .as_str()
            .unwrap()
            .contains("keeper"),
        "{}",
        talk(&absent, 1)
    );
    run_until(&mut absent, 120, |game| !talk(game, 1)["last"].is_null());
    let canceled = talk(&absent, 1)["last"].clone();
    assert_eq!(canceled["given"], false, "{canceled}");
    assert_eq!(talk(&absent, 1)["attendees"], 0);
    assert!(
        lessons(&absent)
            .values()
            .flatten()
            .all(|l| l["source"] != "talk")
    );
    // Released viewers move on: nobody is held at the viewpoint any more.
    absent.tick(60);
    assert_eq!(talk(&absent, 1)["attendees"], 0);

    // Full places: a one-place talk never holds more than one guest.
    let mut full = staffed();
    full.tick(120);
    let start = soon(&full, 15);
    ok(full.schedule_talk(1, start, 20, 1));
    for _ in 0..240 {
        assert!(count(&talk(&full, 1)["attendees"]) <= 1);
        full.tick(1);
    }
    let given = count(&education(&full)["learning"]["total"]["attendees"]);
    assert!(given <= 1, "{given}");

    // Interruption: the keeper is reassigned while on its way or talking; the talk is handed
    // back and, with no keeper left, canceled; places are released and nobody learns from it.
    let mut interrupted = staffed();
    interrupted.tick(120);
    let start = soon(&interrupted, 15);
    ok(interrupted.schedule_talk(1, start, 30, 6));
    run_until(&mut interrupted, 300, |game| {
        matches!(
            talk(game, 1)["status"].as_str(),
            Some("keeper_on_the_way" | "in_progress")
        )
    });
    ok(interrupted.create_work_zone(20, 0, 27, 3));
    ok(interrupted.assign_keeper_zone(1, 2));
    ok(interrupted.assign_keeper_zone(2, 2));
    run_until(&mut interrupted, 120, |game| {
        !talk(game, 1)["last"].is_null()
    });
    assert_eq!(talk(&interrupted, 1)["last"]["given"], false);
    assert_eq!(talk(&interrupted, 1)["attendees"], 0);
    assert!(
        lessons(&interrupted)
            .values()
            .flatten()
            .all(|l| l["source"] != "talk")
    );

    // Cancellation: a talk under way ends at once; the keeper walks back, no place is held.
    let mut canceled = staffed();
    canceled.tick(120);
    let start = soon(&canceled, 15);
    ok(canceled.schedule_talk(1, start, 30, 6));
    run_until(&mut canceled, 300, |game| {
        matches!(
            talk(game, 1)["status"].as_str(),
            Some("keeper_on_the_way" | "in_progress")
        )
    });
    ok(canceled.cancel_talk(1));
    assert!(talk(&canceled, 1).is_null());
    let recent = education(&canceled)["recent_talks"].clone();
    assert_eq!(recent[0]["given"], false, "{recent}");
    assert_eq!(recent[0]["detail"], "Canceled by the park");
    canceled.tick(120);
    assert!(
        lessons(&canceled)
            .values()
            .flatten()
            .all(|l| l["source"] != "talk")
    );
    refused(canceled.cancel_talk(1));

    // Bad schedules change nothing.
    let mut game = living();
    refused(game.schedule_talk(9, 600, 20, 4));
    refused(game.schedule_talk(1, 610, 20, 4));
    refused(game.schedule_talk(1, 600, 5, 4));
    refused(game.schedule_talk(1, 600, 20, 0));
    refused(game.schedule_talk(1, 1440, 20, 4));
    assert!(education(&game)["talks"].as_array().unwrap().is_empty());
}

#[test]
fn donations_and_grants_reconcile_and_cannot_repeat_through_reload_or_a_relocated_sign() {
    let mut game = signed();
    let start = soon(&game, 30);
    ok(game.schedule_talk(1, start, 20, 6));
    ok(game.schedule_talk(2, (start + 60) % 1440, 20, 6));
    run_until(&mut game, 3 * 1440, |game| {
        let view = view(game);
        count(&view["education"]["learning"]["total"]["donors"]) >= 3
            && view["education"]["grants"][0]["awarded_minute"].is_u64()
    });
    assert_ledger_reconciles(&game);
    let grant = education(&game)["grants"][0].clone();
    assert_eq!(grant["id"], "community-learning-grant");
    assert_eq!(cents(&grant["amount_cents"]), 150_000);
    assert_eq!(
        cents(&education(&game)["learning"]["total"]["grants_cents"]),
        150_000
    );
    let donations = education(&game)["donations"].clone();
    assert!(!donations.as_array().unwrap().is_empty(), "{donations}");

    // Reload: nothing is paid again, and the park continues exactly as it would have.
    let save = game.save_json();
    let mut reloaded = ZooGame::new();
    ok(reloaded.load_save(&save));
    game.tick(600);
    reloaded.tick(600);
    assert_eq!(education(&game), education(&reloaded));
    assert_eq!(view(&game)["cash_cents"], view(&reloaded)["cash_cents"]);
    assert_eq!(
        cents(&education(&reloaded)["learning"]["total"]["grants_cents"]),
        150_000,
        "the grant is paid once"
    );
    assert_ledger_reconciles(&reloaded);

    // A relocated sign teaches nobody twice: guests who read it keep their learning when the
    // same sign is rebuilt beside the same path.
    let before = lessons(&game);
    run_until(&mut game, 120, |game| {
        json(&game.evaluate_demolition(3, 11))["ok"] == true
    });
    ok(game.bulldoze(3, 11));
    ok(game.place_concession(3, 9, "sign".to_owned()));
    assert_eq!(sign(&game, 3, 9)["status"], "teaching");
    game.tick(30);
    let after = lessons(&game);
    for (guest, lessons) in &before {
        if let Some(now) = after.get(guest) {
            let points =
                |lessons: &[Value]| -> u64 { lessons.iter().map(|l| count(&l["points"])).sum() };
            // Only a talk can raise what a guest learned at the old sign.
            if now.iter().all(|lesson| lesson["source"] == "sign") {
                assert_eq!(points(now), points(lessons), "guest {guest} learned twice");
            }
        }
    }
    assert_ledger_reconciles(&game);
}

/// The neglected capybaras of the animal-health tests: their keeper leaves out cleaning until
/// they fall ill.
fn neglected() -> ZooGame {
    let mut game = ZooGame::new();
    for x in 2..=14 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 7, 10));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.set_keeper_schedule(1, "feeding,inspection,enrichment".to_owned(), 0, 1440));
    for _ in 0..3 {
        ok(game.buy_animal_feed());
    }
    ok(game.adopt_group(1, "capybara".to_owned(), 2));
    game
}

fn milestone(game: &ZooGame, kind: &str) -> Value {
    education(game)["achievements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|achievement| achievement["kind"] == kind)
        .cloned()
        .unwrap()
}

/// A healthy animal in a habitat at welfare 60+, lowest ID first.
fn healthy_resident(game: &ZooGame) -> u64 {
    let view = view(game);
    let welfare = |habitat: &Value| {
        view["habitats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == *habitat)
            .map_or(0, |entry| count(&entry["welfare"]))
    };
    view["animals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|animal| {
            matches!(
                animal["health"]["status"].as_str(),
                Some("healthy" | "at_risk")
            ) && welfare(&animal["habitat_id"]) >= 60
        })
        .map(|animal| count(&animal["id"]))
        .expect("a healthy, well-kept animal")
}

fn animal_ids(game: &ZooGame) -> Vec<u64> {
    view(game)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|animal| count(&animal["id"]))
        .collect()
}

#[test]
fn conservation_milestones_name_real_eligible_animals_and_resist_adopt_release_loops() {
    // Adopt → release: a fresh adoption sent straight on earns nothing, however often.
    let mut game = staffed();
    ok(game.add_shelter(1));
    for _ in 0..3 {
        let before = animal_ids(&game);
        ok(game.adopt(1, "capybara".to_owned()));
        let newcomer = *animal_ids(&game)
            .iter()
            .find(|id| !before.contains(id))
            .unwrap();
        ok(game.rehome_animal(newcomer as u32));
    }
    let report = education(&game);
    assert!(milestone(&game, "supported_rehoming")["earned"].is_null());
    assert_eq!(report["recent_rehomings"].as_array().unwrap().len(), 3);
    for rehoming in report["recent_rehomings"].as_array().unwrap() {
        assert_eq!(rehoming["eligible"], false);
        assert!(
            rehoming["detail"].as_str().unwrap().contains("new arrival"),
            "{rehoming}"
        );
    }

    // A capybara that lived here three days, healthy and well cared for, earns the milestone,
    // naming that animal; a second one counts but the milestone is earned once.
    care_for(&mut game, 3 * 24);
    let resident = healthy_resident(&game);
    ok(game.rehome_animal(resident as u32));
    let earned = milestone(&game, "supported_rehoming")["earned"].clone();
    assert_eq!(
        earned["animal_ids"],
        serde_json::json!([resident]),
        "{earned}"
    );
    assert_eq!(earned["habitat_id"], 1);
    let second = healthy_resident(&game);
    ok(game.rehome_animal(second as u32));
    let report = education(&game);
    assert_eq!(
        report["achievements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|achievement| !achievement["earned"].is_null())
            .filter(|achievement| achievement["kind"] == "supported_rehoming")
            .count(),
        1
    );
    assert_eq!(milestone(&game, "supported_rehoming")["earned"], earned);

    // A thriving exhibit names the animals present for the whole streak.
    let mut thriving = staffed();
    ok(thriving.add_shelter(1));
    ok(thriving.add_shelter(2));
    for _ in 0..5 * 2 {
        if !milestone(&thriving, "thriving_exhibit")["earned"].is_null() {
            break;
        }
        care_for(&mut thriving, 12);
    }
    assert!(!milestone(&thriving, "thriving_exhibit")["earned"].is_null());
    let earned = milestone(&thriving, "thriving_exhibit")["earned"].clone();
    let habitat = count(&earned["habitat_id"]);
    let residents: Vec<u64> = view(&thriving)["animals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|animal| animal["habitat_id"] == habitat)
        .map(|animal| count(&animal["id"]))
        .collect();
    for id in earned["animal_ids"].as_array().unwrap() {
        assert!(residents.contains(&count(id)), "{earned}");
    }

    // A vet recovery names the recovered animal; the same animal never counts twice.
    let mut sick = neglected();
    run_until(&mut sick, 4000, |game| {
        view(game)["animals"][0]["health"]["status"] == "unwell"
    });
    assert!(milestone(&sick, "vet_recovery")["earned"].is_null());
    assert!(
        milestone(&sick, "vet_recovery")["progress"]
            .as_str()
            .unwrap()
            .contains("is ill"),
    );
    ok(sick.hire_vet());
    ok(sick.buy_medicine());
    ok(sick.set_keeper_schedule(
        1,
        "feeding,cleaning,inspection,enrichment".to_owned(),
        0,
        1440,
    ));
    run_until(&mut sick, 4000, |game| {
        !milestone(game, "vet_recovery")["earned"].is_null()
    });
    let earned = milestone(&sick, "vet_recovery")["earned"].clone();
    assert_eq!(earned["animal_ids"].as_array().unwrap().len(), 1);
    assert!(animal_ids(&sick).contains(&count(&earned["animal_ids"][0])));
}

#[test]
fn save_and_load_preserve_schedules_open_places_eligibility_and_awards() {
    let mut game = signed();
    let start = soon(&game, 30);
    ok(game.schedule_talk(1, start, 30, 6));
    // Save while the talk is open, with places reserved.
    run_until(&mut game, 3 * 1440, |game| {
        count(&talk(game, 1)["attendees"]) > 0
    });
    let save = game.save_json();
    let mut reloaded = ZooGame::new();
    ok(reloaded.load_save(&save));
    assert_eq!(education(&game), education(&reloaded));
    assert_eq!(json(&reloaded.save_json()), json(&save));
    for _ in 0..8 {
        game.tick(240);
        reloaded.tick(240);
        assert_eq!(education(&game), education(&reloaded));
    }
    assert!(count(&education(&reloaded)["learning"]["total"]["talks_given"]) >= 1);

    // Earned milestones and their evidence survive a reload unchanged.
    ok(game.add_shelter(1));
    ok(game.add_shelter(2));
    ok(game.hire_vet());
    ok(game.buy_medicine());
    care_for(&mut game, 3 * 24);
    let resident = healthy_resident(&game);
    ok(game.rehome_animal(resident as u32));
    let earned = milestone(&game, "supported_rehoming")["earned"].clone();
    assert!(
        !earned.is_null(),
        "{}",
        education(&game)["recent_rehomings"]
    );
    let mut again = ZooGame::new();
    ok(again.load_save(&game.save_json()));
    assert_eq!(milestone(&again, "supported_rehoming")["earned"], earned);
    assert_eq!(education(&again)["talks"], education(&game)["talks"]);
}

/// The park the browser proof opens (`apps/web/scripts/education-proof.mjs`).
const PROOF_PARK: &str = "tests/fixtures/education-park.json";

/// The lived-in starter park with a second keeper, half an hour into its first day with guests on
/// their way: no signs or talks yet, and nobody taught.
fn proof_park() -> ZooGame {
    let mut game = staffed();
    game.tick(30);
    game
}

/// The browser proof's park must stay a current, loadable save; rewrite it with
/// `ZOO_WRITE_EDUCATION_PARK=1`.
#[test]
fn the_education_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_EDUCATION_PARK").is_some() {
        let value = json(&proof_park().save_json());
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
    let report = education(&game);
    assert_eq!(report["learning"]["total"]["guests_educated"], 0);
    assert_eq!(report["grants"][0]["id"], "community-learning-grant");
    assert!(report["grants"][0]["awarded_minute"].is_null());
}
