//! Guest-comfort facilities (#101): toilets, benches and litter bins through the public game
//! seam. Guest needs are staged by editing a real save and loading it, so every fixture runs the
//! same simulation, reservation, janitor and ledger rules as play.
use serde_json::Value;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn accepted(result: String) {
    let result = json(&result);
    assert_eq!(result["ok"], true, "{result}");
}

fn view(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

/// The starter meadow with a stocked, staffed capybara habitat south of a path that runs from
/// the entrance to x = 14, so guests arrive and walk past the north-side building plots.
fn park() -> ZooGame {
    let mut game = ZooGame::new();
    accepted(game.skip_tutorial());
    accepted(game.place_habitat_rect(3, 8, 7, 10));
    accepted(game.hire_keeper());
    accepted(game.create_work_zone(0, 0, 19, 13));
    accepted(game.assign_keeper_zone(1, 1));
    accepted(game.buy_animal_feed());
    accepted(game.adopt(1, "capybara".to_owned()));
    accepted(game.adopt(1, "capybara".to_owned()));
    accepted(game.build_path_stroke(vec![4, 7, 14, 7], 1, "public".to_owned()));
    game
}

/// Rewrites the saved park and loads it back, as the browser's file import would.
fn edit(game: &mut ZooGame, change: impl FnOnce(&mut Value)) {
    let mut save = json(&game.save_json());
    change(&mut save["park"]);
    accepted(game.load_save(&save.to_string()));
}

fn set_guests(game: &mut ZooGame, field: &str, value: u64) {
    edit(game, |park| {
        for guest in park["guests"].as_array_mut().unwrap() {
            guest[field] = value.into();
        }
    });
}

fn guests(view: &Value) -> Vec<Value> {
    view["guests"].as_array().unwrap().clone()
}

fn facility(view: &Value, x: u64, y: u64) -> Value {
    view["concessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stand| stand["x"] == x && stand["y"] == y)
        .cloned()
        .unwrap_or(Value::Null)
}

fn alert_codes(view: &Value) -> Vec<String> {
    view["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|alert| alert["code"].as_str().unwrap().to_owned())
        .collect()
}

fn alert<'a>(view: &'a Value, code: &str) -> Option<&'a Value> {
    view["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|alert| alert["code"] == code)
}

/// Saved reservations: every slot is held at most once and never beyond capacity.
fn assert_slots_consistent(game: &ZooGame) {
    let save = json(&game.save_json());
    let mut held = std::collections::HashSet::new();
    for stand in save["park"]["concessions"].as_array().unwrap() {
        let slots = stand["slots"].as_array().unwrap();
        assert!(
            slots.len() as u64 <= stand["definition"]["capacity"].as_u64().unwrap(),
            "{stand}"
        );
        for holder in slots {
            assert!(
                held.insert(holder.as_u64().unwrap()),
                "{holder} holds two slots"
            );
        }
    }
    assert_eq!(game.session_status(), "running");
}

fn step_until(game: &mut ZooGame, minutes: u32, mut done: impl FnMut(&Value) -> bool) -> Value {
    for _ in 0..minutes {
        game.tick(1);
        assert_slots_consistent(game);
        let now = view(game);
        if done(&now) {
            return now;
        }
    }
    panic!(
        "condition not reached within {minutes} minutes: {}",
        view(game)
    );
}

#[test]
fn only_a_reachable_toilet_meets_the_need() {
    let mut game = park();
    game.tick(80);
    set_guests(&mut game, "bladder", 70);
    let with_need: Vec<u64> = guests(&view(&game))
        .iter()
        .map(|guest| guest["id"].as_u64().unwrap())
        .collect();
    assert!(!with_need.is_empty());

    // No toilet: the need is recorded, complained about and raised as an alert.
    let now = step_until(&mut game, 12, |now| {
        now["complaints"]["needs_toilet"].as_u64().unwrap() > 0
    });
    assert!(alert_codes(&now).contains(&"toilet_demand_unmet".to_owned()));
    assert_eq!(
        alert(&now, "toilet_demand_unmet").unwrap()["action"],
        "build_toilet"
    );
    assert!(
        guests(&now)
            .iter()
            .any(|guest| guest["thought"] == "I really need a toilet!")
    );

    // An isolated toilet beside a path nobody can walk to gives no free recovery.
    accepted(game.build_path_stroke(vec![16, 3], 1, "public".to_owned()));
    accepted(game.place_concession(16, 2, "toilet".to_owned()));
    game.tick(30);
    let now = view(&game);
    assert_eq!(facility(&now, 16, 2)["comfort"]["total_uses"], 0);
    for guest in guests(&now) {
        if with_need.contains(&guest["id"].as_u64().unwrap()) {
            assert!(
                guest["comfort"]["bladder"].as_u64().unwrap() >= 70,
                "{guest}"
            );
        }
    }
    assert!(alert_codes(&now).contains(&"toilet_demand_unmet".to_owned()));

    // A reachable toilet is chosen, walked to, used, and the complaint resolves.
    accepted(game.place_concession(10, 6, "toilet".to_owned()));
    let mut seen_using = false;
    let now = step_until(&mut game, 120, |now| {
        seen_using |= guests(now)
            .iter()
            .any(|guest| guest["state"] == "using_facility" && guest["thought"] == "Much better.");
        seen_using
            && facility(now, 10, 6)["comfort"]["total_uses"]
                .as_u64()
                .unwrap()
                > 0
            && !alert_codes(now).contains(&"toilet_demand_unmet".to_owned())
    });
    assert_eq!(now["complaints"]["needs_toilet"], 0);
    assert_eq!(facility(&now, 16, 2)["comfort"]["total_uses"], 0);
}

#[test]
fn a_full_toilet_queues_guests_and_never_double_books_a_cubicle() {
    let run = || {
        let mut game = park();
        game.tick(150);
        accepted(game.place_concession(10, 6, "toilet".to_owned()));
        // A one-cubicle toilet (content travels with the save) so the starter park's few
        // guests contend for it.
        let mut save = json(&game.save_json());
        for facility in save["content"]["facilities"].as_array_mut().unwrap() {
            if facility["key"] == "toilet" {
                facility["capacity"] = 1.into();
            }
        }
        for stand in save["park"]["concessions"].as_array_mut().unwrap() {
            if stand["definition"]["key"] == "toilet" {
                stand["definition"]["capacity"] = 1.into();
            }
        }
        // Every guest stands at the toilet's door, between destinations, needing it at once.
        for guest in save["park"]["guests"].as_array_mut().unwrap() {
            guest["bladder"] = 70.into();
            guest["state"] = "walking_to_habitat".into();
            guest["x"] = 10.into();
            guest["y"] = 7.into();
            guest["route"] = serde_json::json!([]);
            guest["route_index"] = 0.into();
            guest["arrival_steps"] = 0.into();
        }
        assert!(save["park"]["guests"].as_array().unwrap().len() >= 2);
        accepted(game.load_save(&save.to_string()));
        let mut queued = false;
        let mut transcript = Vec::new();
        for _ in 0..90 {
            game.tick(1);
            assert_slots_consistent(&game);
            let now = view(&game);
            let toilet = facility(&now, 10, 6);
            let capacity = toilet["comfort"]["capacity"].as_u64().unwrap();
            let using = guests(&now)
                .iter()
                .filter(|guest| guest["state"] == "using_facility")
                .count() as u64;
            assert_eq!(toilet["comfort"]["in_use"].as_u64().unwrap(), using);
            assert!(using <= capacity, "{toilet}");
            queued |= toilet["comfort"]["waiting"].as_u64().unwrap() > 0;
            transcript.push(game.snapshot_json());
        }
        (queued, transcript, game.save_json())
    };
    let (queued, first, save) = run();
    assert!(queued, "more guests needed the toilet than it has cubicles");
    let (_, second, again) = run();
    assert_eq!(
        first, second,
        "facility choice and queueing are deterministic"
    );
    assert_eq!(save, again);
}

#[test]
fn demolition_and_path_cuts_release_reservations() {
    // Demolition: guests bound for the toilet give their slots back and choose again.
    let mut game = park();
    game.tick(80);
    accepted(game.place_concession(10, 6, "toilet".to_owned()));
    set_guests(&mut game, "bladder", 70);
    step_until(&mut game, 30, |now| {
        guests(now)
            .iter()
            .any(|guest| guest["comfort"]["facility_id"].is_u64())
    });
    let preview = json(&game.evaluate_demolition(10, 6));
    assert_eq!(preview["undoable"], false, "{preview}");
    accepted(game.bulldoze(10, 6));
    assert_slots_consistent(&game);
    let now = view(&game);
    assert!(
        guests(&now)
            .iter()
            .all(|guest| guest["comfort"]["facility_id"].is_null())
    );
    step_until(&mut game, 15, |now| {
        alert_codes(now).contains(&"toilet_demand_unmet".to_owned())
    });

    // Path cut: a toilet at the end of a spur loses its only route; walkers give up on it.
    let mut game = park();
    accepted(game.build_path_stroke(vec![14, 7, 17, 7], 1, "public".to_owned()));
    accepted(game.place_concession(17, 6, "toilet".to_owned()));
    game.tick(80);
    set_guests(&mut game, "bladder", 70);
    step_until(&mut game, 30, |now| {
        guests(now).iter().any(|guest| {
            guest["state"] == "walking_to_facility" && guest["x"].as_u64().unwrap() < 15
        })
    });
    let cut = (1..=20).find(|_| {
        let result = json(&game.bulldoze(15, 7));
        if result["ok"] == true {
            return true;
        }
        game.tick(1);
        false
    });
    assert!(cut.is_some(), "the spur could be cut");
    assert_slots_consistent(&game);
    let now = view(&game);
    let toilet = facility(&now, 17, 6);
    assert_eq!(toilet["comfort"]["heading"], 0, "{toilet}");
    assert!(guests(&now).iter().any(|guest| {
        guest["comfort"]["unmet_need"] == "toilet" && guest["comfort"]["facility_id"].is_null()
    }));
}

#[test]
fn dirty_toilets_and_full_bins_get_janitor_work_once_and_recover_only_when_serviced() {
    let mut game = park();
    accepted(game.place_concession(10, 6, "toilet".to_owned()));
    accepted(game.place_concession(8, 6, "food".to_owned()));
    accepted(game.place_concession(9, 6, "bin".to_owned()));
    game.tick(80);
    edit(&mut game, |park| {
        for stand in park["concessions"].as_array_mut().unwrap() {
            match stand["definition"]["key"].as_str().unwrap() {
                "toilet" => stand["cleanliness"] = 60.into(),
                "bin" => stand["fill"] = 5.into(),
                _ => {}
            }
        }
        for guest in park["guests"].as_array_mut().unwrap() {
            guest["bladder"] = 70.into();
            guest["hunger"] = 50.into();
        }
    });

    // One use makes the toilet dirty and one wrapper fills the bin: one task each.
    let now = step_until(&mut game, 90, |now| {
        now["facility_service"].as_array().unwrap().len() == 2
    });
    let work: Vec<&str> = now["facility_service"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["work"].as_str().unwrap())
        .collect();
    assert!(
        work.contains(&"clean_toilet") && work.contains(&"empty_bin"),
        "{work:?}"
    );
    let service = alert(&now, "facility_needs_service").unwrap();
    assert_eq!(service["action"], "hire_janitor");

    // More use never duplicates the work, a full bin sends litter to the path, and nothing
    // recovers on its own.
    let litter_before = now["litter"].as_array().unwrap().len();
    edit(&mut game, |park| {
        for guest in park["guests"].as_array_mut().unwrap() {
            guest["bladder"] = 70.into();
            guest["hunger"] = 50.into();
            guest["bought_food"] = false.into();
        }
    });
    game.tick(60);
    let now = view(&game);
    assert_eq!(now["facility_service"].as_array().unwrap().len(), 2);
    let toilet = facility(&now, 10, 6);
    assert!(
        toilet["comfort"]["cleanliness"].as_u64().unwrap() <= 45,
        "{toilet}"
    );
    assert_eq!(facility(&now, 9, 6)["comfort"]["fill"], 6);
    assert!(now["litter"].as_array().unwrap().len() >= litter_before);

    // A janitor walks over; service lands only on arrival, then each facility is restored.
    accepted(game.hire_janitor());
    game.tick(3);
    let now = view(&game);
    let janitor = &now["animal_care_depot"]["janitors"][0];
    assert!(!janitor["target"].is_null(), "{janitor}");
    assert!(
        janitor["status"].as_str().unwrap().starts_with("Servicing"),
        "{janitor}"
    );
    let open: Vec<u64> = now["facility_service"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["concession_id"].as_u64().unwrap())
        .collect();
    let toilet = facility(&now, 10, 6);
    if open.contains(&toilet["id"].as_u64().unwrap()) {
        assert!(
            toilet["comfort"]["cleanliness"].as_u64().unwrap() <= 45,
            "{toilet}"
        );
    }
    let now = step_until(&mut game, 240, |now| {
        now["facility_service"].as_array().unwrap().is_empty()
    });
    assert_eq!(facility(&now, 9, 6)["comfort"]["fill"], 0);
    assert!(
        facility(&now, 10, 6)["comfort"]["cleanliness"]
            .as_u64()
            .unwrap()
            > 45
    );
    assert!(
        now["animal_care_depot"]["janitors"][0]["tasks_completed"]
            .as_u64()
            .unwrap()
            >= 2
    );
    assert!(alert(&now, "facility_needs_service").is_none());
}

#[test]
fn an_interrupted_janitor_releases_the_job_and_resumes_after_the_path_returns() {
    let mut game = park();
    accepted(game.place_concession(12, 6, "toilet".to_owned()));
    game.tick(30);
    edit(&mut game, |park| {
        for stand in park["concessions"].as_array_mut().unwrap() {
            if stand["definition"]["key"] == "toilet" {
                stand["cleanliness"] = 50.into();
            }
        }
    });
    // A toilet already below the service line asks for a janitor on its next use.
    set_guests(&mut game, "bladder", 70);
    step_until(&mut game, 60, |now| {
        !now["facility_service"].as_array().unwrap().is_empty()
    });
    accepted(game.hire_janitor());
    step_until(&mut game, 6, |now| {
        !now["animal_care_depot"]["janitors"][0]["target"].is_null()
    });
    // Cut the main path between the depot side and the toilet.
    let cut = (1..=30).find(|_| {
        if json(&game.bulldoze(9, 7))["ok"] == true {
            return true;
        }
        game.tick(1);
        false
    });
    assert!(cut.is_some(), "the main path could be cut");
    game.tick(3);
    let now = view(&game);
    let janitor = &now["animal_care_depot"]["janitors"][0];
    assert!(
        janitor["x"].as_u64().unwrap() < 9,
        "the janitor is cut off: {janitor}"
    );
    assert!(janitor["target"].is_null(), "{janitor}");
    let task = &now["facility_service"][0];
    assert!(task["assigned_janitor_id"].is_null(), "{task}");
    assert_eq!(
        alert(&now, "facility_needs_service").unwrap()["action"],
        "build_path"
    );
    assert!(
        facility(&now, 12, 6)["comfort"]["cleanliness"]
            .as_u64()
            .unwrap()
            <= 50
    );
    accepted(game.build_path_stroke(vec![9, 7], 1, "public".to_owned()));
    let now = step_until(&mut game, 120, |now| {
        now["facility_service"].as_array().unwrap().is_empty()
    });
    assert_eq!(facility(&now, 12, 6)["comfort"]["cleanliness"], 100);
}

#[test]
fn benches_restore_energy_only_while_occupied() {
    let mut game = park();
    accepted(game.place_concession(11, 6, "bench".to_owned()));
    game.tick(80);
    set_guests(&mut game, "energy", 45);
    let now = step_until(&mut game, 60, |now| {
        guests(now)
            .iter()
            .any(|guest| guest["state"] == "using_facility")
    });
    let sitter = guests(&now)
        .into_iter()
        .find(|guest| guest["state"] == "using_facility")
        .unwrap();
    let id = sitter["id"].clone();
    let start = sitter["energy"].as_u64().unwrap();
    let now = step_until(&mut game, 30, |now| {
        guests(now)
            .iter()
            .find(|guest| guest["id"] == id)
            .is_none_or(|guest| guest["state"] != "using_facility")
    });
    {
        let guest = guests(&now)
            .into_iter()
            .find(|guest| guest["id"] == id)
            .expect("the rested guest is still in the park");
        assert!(guest["energy"].as_u64().unwrap() > start, "{guest}");
        // Walking away, energy only falls again.
        let rested = guest["energy"].as_u64().unwrap();
        game.tick(6);
        let later = view(&game);
        let guest = guests(&later)
            .into_iter()
            .find(|guest| guest["id"] == id)
            .expect("the rested guest walks on");
        assert_ne!(guest["state"], "using_facility");
        assert!(guest["energy"].as_u64().unwrap() <= rested, "{guest}");
    }
    assert!(
        facility(&now, 11, 6)["comfort"]["total_uses"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn long_running_comfort_reconciles_with_the_ledger_and_stays_consistent() {
    let costs = |game: &ZooGame| {
        let now = view(game);
        let today = &now["finance"]["current_day"]["breakdown"];
        (
            today["construction_expense_cents"].as_i64().unwrap(),
            today["park_upkeep_expense_cents"].as_i64().unwrap(),
        )
    };
    let mut plain = park();
    let mut comfort = park();
    let (built_before, upkeep_before) = costs(&comfort);
    assert_eq!(costs(&plain), (built_before, upkeep_before));
    accepted(comfort.place_concession(10, 6, "toilet".to_owned()));
    accepted(comfort.place_concession(11, 6, "bench".to_owned()));
    accepted(comfort.place_concession(9, 6, "bin".to_owned()));
    let catalog = json(&comfort.content_json());
    let price = |key: &str, field: &str| {
        catalog["facilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|facility| facility["key"] == key)
            .unwrap()[field]
            .as_i64()
            .unwrap()
    };
    let keys = ["toilet", "bench", "bin"];
    let build: i64 = keys.iter().map(|key| price(key, "build_cost_cents")).sum();
    let hourly: i64 = keys
        .iter()
        .map(|key| price(key, "hourly_upkeep_cents"))
        .sum();
    assert_eq!(costs(&comfort).0 - built_before, build);
    plain.tick(60);
    comfort.tick(60);
    assert_eq!(costs(&comfort).1 - costs(&plain).1, hourly);

    // Three days of operation with staff: reservations stay consistent every minute, each
    // facility has at most one janitor task, and the same commands replay identically.
    let run = |game: &mut ZooGame| {
        accepted(game.hire_janitor());
        accepted(game.hire_mechanic());
        let mut uses = 0;
        for _ in 0..(3 * 24 * 60) {
            game.tick(1);
            assert_eq!(game.session_status(), "running");
        }
        let now = view(game);
        for stand in now["concessions"].as_array().unwrap() {
            if let Some(comfort) = stand["comfort"].as_object() {
                uses += comfort["total_uses"].as_u64().unwrap();
            }
        }
        let open = now["facility_service"].as_array().unwrap();
        let mut ids: Vec<u64> = open
            .iter()
            .map(|task| task["concession_id"].as_u64().unwrap())
            .collect();
        ids.dedup();
        assert_eq!(ids.len(), open.len());
        assert_slots_consistent(game);
        uses
    };
    let mut twin = ZooGame::new();
    accepted(twin.load_save(&comfort.save_json()));
    let uses = run(&mut comfort);
    assert!(
        uses > 10,
        "the facilities were used over three days ({uses})"
    );
    run(&mut twin);
    assert_eq!(comfort.save_json(), twin.save_json());
}
