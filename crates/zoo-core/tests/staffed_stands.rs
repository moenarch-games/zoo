//! Staffed food and drink stands (#102) through the public game seam. Guest needs, stock and
//! service models are staged by editing a real save and loading it, so every fixture runs the
//! same choice, queue, service, sale, restocking and ledger rules as play.
use serde_json::Value;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn accepted(result: String) -> Value {
    let result = json(&result);
    assert_eq!(result["ok"], true, "{result}");
    result
}

fn view(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

/// A food stand north of the path's far end: guests visiting the habitat never pass it.
const STAND: (u32, u32) = (13, 6);

/// The starter meadow with a stocked, staffed capybara habitat south of a path that runs from
/// the entrance to x = 14, and a food stand off the guests' way at its far end.
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
    accepted(game.place_concession(STAND.0, STAND.1, "food".to_owned()));
    // A mechanic keeps the stand from wearing out during long runs (#57).
    accepted(game.hire_mechanic());
    game
}

/// The park after guests have arrived, all hungry enough to make a detour.
fn hungry_park(arrival_minutes: u32) -> ZooGame {
    let mut game = park();
    game.tick(arrival_minutes);
    set_guests(&mut game, "hunger", 60);
    game
}

/// Rewrites the saved park (and content) and loads it back, as the browser's import would.
fn edit(game: &mut ZooGame, change: impl FnOnce(&mut Value)) {
    let mut save = json(&game.save_json());
    change(&mut save);
    accepted(game.load_save(&save.to_string()));
}

/// Adds `count` more guests where an idle guest stands, as a busier day would bring them.
fn add_guests(game: &mut ZooGame, count: u64) {
    edit(game, |save| {
        let park = &mut save["park"];
        let template = park["guests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|guest| guest["visit"].is_null() && guest["state"] != "arriving")
            .expect("an idle guest")
            .clone();
        let next = park["next_guest_id"].as_u64().unwrap();
        for offset in 0..count {
            let mut guest = template.clone();
            guest["id"] = (next + offset).into();
            guest["state"] = "walking_to_habitat".into();
            guest["route"] = serde_json::json!([]);
            guest["route_index"] = 0.into();
            guest["cash_cents"] = 2_000.into();
            park["guests"].as_array_mut().unwrap().push(guest);
        }
        park["next_guest_id"] = (next + count).into();
    });
}

fn set_guests(game: &mut ZooGame, field: &str, value: i64) {
    edit(game, |save| {
        for guest in save["park"]["guests"].as_array_mut().unwrap() {
            guest[field] = value.into();
        }
    });
}

/// Changes the food stand's service model in the catalog and in every built food stand.
fn set_service(game: &mut ZooGame, field: &str, value: u64) {
    edit(game, |save| {
        for facility in save["content"]["facilities"].as_array_mut().unwrap() {
            if facility["key"] == "food" {
                facility["stand"][field] = value.into();
            }
        }
        for stand in save["park"]["concessions"].as_array_mut().unwrap() {
            if stand["definition"]["key"] == "food" {
                stand["definition"]["stand"][field] = value.into();
            }
        }
    });
}

/// Changes the first food stand's saved operating state.
fn set_stand_state(game: &mut ZooGame, field: &str, value: Value) {
    edit(game, |save| {
        let stand = save["park"]["concessions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|stand| stand["definition"]["key"] == "food")
            .unwrap();
        stand["stand"][field] = value;
    });
}

fn stand(view: &Value, x: u32, y: u32) -> Value {
    view["concessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stand| stand["x"] == x && stand["y"] == y)
        .cloned()
        .unwrap_or(Value::Null)
}

fn food(view: &Value) -> Value {
    stand(view, STAND.0, STAND.1)
}

fn number(value: &Value) -> i64 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

fn alert_codes(view: &Value) -> Vec<String> {
    view["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|alert| alert["code"].as_str().unwrap().to_owned())
        .collect()
}

/// Saved stand places: queues and windows hold distinct guests within the service model and
/// stock is never negative or above capacity; the session stays healthy.
fn assert_stands_consistent(game: &ZooGame) {
    assert_eq!(game.session_status(), "running");
    let save = json(&game.save_json());
    let mut placed = std::collections::HashSet::new();
    for stand in save["park"]["concessions"].as_array().unwrap() {
        let Some(ops) = stand["stand"].as_object() else {
            continue;
        };
        let service = &stand["definition"]["stand"];
        let stock = number(&ops["stock"]);
        assert!(
            (0..=number(&service["stock_capacity"])).contains(&stock),
            "{stand}"
        );
        let queue = ops["queue"].as_array().unwrap();
        let slots = stand["slots"].as_array().unwrap();
        assert!(queue.len() as i64 <= number(&service["queue_capacity"]));
        assert!(slots.len() as i64 <= number(&service["max_vendors"]));
        for guest in queue.iter().chain(slots) {
            assert!(
                placed.insert(guest.as_u64().unwrap()),
                "{guest} placed twice"
            );
        }
    }
}

fn step_until(game: &mut ZooGame, minutes: u32, mut done: impl FnMut(&Value) -> bool) -> Value {
    for _ in 0..minutes {
        game.tick(1);
        assert_stands_consistent(game);
        let now = view(game);
        if done(&now) {
            return now;
        }
    }
    panic!(
        "condition not reached within {minutes} minutes: {}",
        food(&view(game))
    );
}

#[test]
fn a_hungry_guest_detours_to_a_stand_it_would_never_pass() {
    let mut game = hungry_park(90);
    let mut detoured = false;
    let now = step_until(&mut game, 120, |now| {
        detoured |= now["guests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guest| guest["thought"] == "I'm off to get something to eat.");
        detoured && number(&food(now)["total_sales"]) > 0
    });
    let stand = food(&now);
    assert_eq!(stand["stand"]["status"], "open");
    assert_eq!(
        number(&stand["stand"]["today"]["served"]),
        number(&stand["total_sales"])
    );
    assert_eq!(
        number(&stand["stand"]["stock"]),
        number(&stand["stand"]["stock_capacity"]) - number(&stand["total_sales"])
    );
    // Guests who were only passing never come near: without hunger, nobody buys.
    let mut calm = park();
    for _ in 0..4 {
        calm.tick(30);
        set_guests(&mut calm, "hunger", 0);
    }
    assert_eq!(number(&food(&view(&calm))["total_sales"]), 0);
}

#[test]
fn queues_serve_first_come_first_served_and_turn_guests_away_when_full() {
    let mut game = hungry_park(180);
    set_service(&mut game, "service_minutes", 10);
    set_service(&mut game, "queue_capacity", 3);
    add_guests(&mut game, 6);
    set_guests(&mut game, "hunger", 60);
    let guests = view(&game)["guests"].as_array().unwrap().len();
    assert!(guests >= 5, "{guests} guests");
    let places = |game: &ZooGame| {
        let save = json(&game.save_json());
        let stand = save["park"]["concessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|stand| stand["definition"]["key"] == "food")
            .unwrap()
            .clone();
        let ids = |value: &Value| -> Vec<u64> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_u64().unwrap())
                .collect()
        };
        (ids(&stand["stand"]["queue"]), ids(&stand["slots"]))
    };
    let (mut queue, mut slots) = places(&game);
    let mut served_from_queue = 0;
    let mut longest = 0;
    for _ in 0..180 {
        game.tick(1);
        assert_stands_consistent(&game);
        let (next_queue, next_slots) = places(&game);
        // Guests still waiting keep their order at the front; newcomers join at the back.
        let staying: Vec<u64> = queue
            .iter()
            .copied()
            .filter(|id| next_queue.contains(id))
            .collect();
        assert_eq!(
            next_queue[..staying.len()],
            staying[..],
            "{queue:?} -> {next_queue:?}"
        );
        // Nobody is served while someone ahead of them still waits.
        for (position, id) in queue.iter().enumerate() {
            if next_slots.contains(id) && !slots.contains(id) {
                served_from_queue += 1;
                assert!(
                    queue[..position]
                        .iter()
                        .all(|ahead| !next_queue.contains(ahead)),
                    "{id} was served before someone ahead of it: {queue:?} -> {next_queue:?}"
                );
            }
        }
        longest = longest.max(next_queue.len());
        (queue, slots) = (next_queue, next_slots);
    }
    assert!(
        served_from_queue >= 2,
        "{served_from_queue} served from the queue"
    );
    assert_eq!(longest, 3, "the queue filled");
    let lost = &food(&view(&game))["stand"]["total"]["lost"];
    assert!(number(&lost["queue_full"]) > 0, "{lost}");
    assert!(number(&lost["gave_up"]) > 0, "{lost}");
}

#[test]
fn an_unaffordable_purchase_changes_no_money_stock_or_need() {
    let mut game = hungry_park(90);
    set_guests(&mut game, "cash_cents", 100);
    let before = view(&game);
    let cash = number(&before["cash_cents"]);
    game.tick(60);
    let now = view(&game);
    let stand = food(&now);
    assert_eq!(number(&stand["total_sales"]), 0);
    assert_eq!(stand["stand"]["stock"], stand["stand"]["stock_capacity"]);
    assert!(number(&stand["stand"]["total"]["lost"]["unaffordable"]) > 0);
    assert_eq!(
        now["finance"]["current_day"]["breakdown"]["concession_income_cents"],
        0
    );
    for guest in now["guests"].as_array().unwrap() {
        if number(&guest["id"]) <= number(&before["entrance"]["arrivals_total"]) {
            assert_eq!(number(&guest["cash_cents"]), 100, "{guest}");
            assert!(number(&guest["hunger"]) >= 60, "{guest}");
        }
    }
    assert!(
        now["guests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guest| !guest["comfort"]["unmet_need"].is_null())
    );
    // Admission is the only income since; nothing was earned at the stand.
    assert!(number(&now["cash_cents"]) <= cash + 5 * 1_200);
}

#[test]
fn stock_runs_out_without_going_negative_and_a_paid_delivery_refills_it() {
    let mut game = hungry_park(150);
    set_stand_state(&mut game, "stock", 2.into());
    set_stand_state(&mut game, "auto_restock", false.into());
    let now = step_until(&mut game, 120, |now| {
        food(now)["stand"]["status"] == "out_of_stock"
    });
    let stand = food(&now);
    assert_eq!(number(&stand["total_sales"]), 2);
    assert_eq!(number(&stand["stand"]["stock"]), 0);
    assert!(alert_codes(&now).contains(&"stand_out_of_stock".to_owned()));
    // Hungry guests are now turned away, deterministically and without a sale.
    set_guests(&mut game, "hunger", 60);
    let now = step_until(&mut game, 60, |now| {
        number(&food(now)["stand"]["total"]["lost"]["out_of_stock"]) > 0
    });
    assert_eq!(number(&food(&now)["total_sales"]), 2);

    // A delivery the park cannot pay for brings nothing and costs nothing.
    let cash = number(&now["cash_cents"]);
    edit(&mut game, |save| save["park"]["cash_cents"] = 0.into());
    let ordered = accepted(game.restock_stand(1));
    assert!(
        ordered["message"]
            .as_str()
            .unwrap()
            .contains("arrives in 10 min")
    );
    assert_eq!(food(&view(&game))["stand"]["status"], "restocking");
    let repeated = accepted(game.restock_stand(1));
    assert!(repeated["message"].as_str().unwrap().contains("already"));
    game.tick(11);
    let now = view(&game);
    assert_eq!(number(&food(&now)["stand"]["stock"]), 0);
    assert_eq!(
        now["finance"]["current_day"]["breakdown"]["stand_stock_expense_cents"],
        0
    );

    // A paid delivery refills to full at the unit cost, on both ledgers.
    edit(&mut game, |save| save["park"]["cash_cents"] = cash.into());
    accepted(game.restock_stand(1));
    game.tick(11);
    let now = view(&game);
    let stand = food(&now);
    let capacity = number(&stand["stand"]["stock_capacity"]);
    let unit = number(&stand["stand"]["stock_unit_cost_cents"]);
    let sold_since = number(&stand["total_sales"]) - 2;
    assert_eq!(number(&stand["stand"]["stock"]), capacity - sold_since);
    assert_eq!(
        number(&now["finance"]["current_day"]["breakdown"]["stand_stock_expense_cents"]),
        capacity * unit
    );
    assert_eq!(
        number(&stand["stand"]["today"]["stock_cost_cents"]),
        capacity * unit
    );
    assert!(!alert_codes(&now).contains(&"stand_out_of_stock".to_owned()));
}

#[test]
fn vendor_absence_closing_and_failure_send_guests_away_with_distinct_reasons() {
    let run = || {
        let mut game = hungry_park(150);
        set_service(&mut game, "service_minutes", 10);
        set_guests(&mut game, "hunger", 60);
        // Someone is being served while someone else queues.
        step_until(&mut game, 120, |now| {
            let ops = &food(now)["stand"];
            number(&ops["serving"]) > 0 && number(&ops["queue"]) > 0
        });
        let hunger: Vec<i64> = view(&game)["guests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|guest| number(&guest["hunger"]))
            .collect();
        let sales = number(&food(&view(&game))["total_sales"]);

        // No vendor: every guest at the stand leaves without a sale or a meal.
        accepted(game.set_stand_vendors(1, 0));
        let now = view(&game);
        let ops = &food(&now)["stand"];
        assert_eq!(ops["status"], "unstaffed");
        assert_eq!(ops["status_label"], "Closed · no vendor");
        assert_eq!(number(&ops["serving"]) + number(&ops["queue"]), 0);
        assert!(number(&ops["total"]["lost"]["unstaffed"]) >= 2);
        assert_eq!(number(&food(&now)["total_sales"]), sales);
        let after: Vec<i64> = now["guests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|guest| number(&guest["hunger"]))
            .collect();
        assert_eq!(hunger, after, "no need recovered without a sale");
        assert!(alert_codes(&now).contains(&"stand_unstaffed".to_owned()));
        assert!(
            accepted(game.set_stand_vendors(1, 0))["message"]
                .as_str()
                .unwrap()
                .contains("already")
        );
        assert_eq!(json(&game.set_stand_vendors(1, 4))["ok"], false);

        // Back to work, then closed: the queue leaves, service in progress finishes.
        accepted(game.set_stand_vendors(1, 1));
        set_guests(&mut game, "hunger", 60);
        step_until(&mut game, 120, |now| {
            let ops = &food(now)["stand"];
            number(&ops["serving"]) > 0 && number(&ops["queue"]) > 0
        });
        let sales = number(&food(&view(&game))["total_sales"]);
        accepted(game.set_stand_open(1, false));
        let now = view(&game);
        assert_eq!(food(&now)["stand"]["status"], "closed");
        assert_eq!(number(&food(&now)["stand"]["queue"]), 0);
        assert!(number(&food(&now)["stand"]["total"]["lost"]["closed"]) > 0);
        game.tick(15);
        assert_eq!(number(&food(&view(&game))["total_sales"]), sales + 1);

        // Broken down (#57): closed with its own reason until a mechanic repairs it.
        accepted(game.set_stand_open(1, true));
        edit(&mut game, |save| {
            save["park"]["concessions"][0]["condition"] = 15.into();
        });
        set_guests(&mut game, "hunger", 60);
        let repairs = number(&food(&view(&game))["stand"]["total"]["repair_cents"]);
        let now = step_until(&mut game, 60, |now| {
            number(&food(now)["stand"]["total"]["lost"]["broken_down"]) > 0
        });
        assert_eq!(food(&now)["stand"]["status"], "broken_down");
        assert_eq!(food(&now)["stand"]["status_label"], "Closed · broken down");
        assert!(alert_codes(&now).contains(&"facility_failed".to_owned()));
        // The park's mechanic repairs it, and the stand's ledger carries the repair.
        let now = step_until(&mut game, 240, |now| food(now)["stand"]["status"] == "open");
        assert_eq!(
            number(&food(&now)["stand"]["total"]["repair_cents"]) - repairs,
            number(&json(&game.content_json())["balance"]["maintenance_repair_cents"])
        );
        game.save_json()
    };
    // The same commands give the same queues, reasons and outcomes.
    assert_eq!(run(), run());
}

#[test]
fn demolition_path_cuts_and_reloads_never_duplicate_money_stock_or_places() {
    let busy = || {
        let mut game = hungry_park(150);
        set_service(&mut game, "service_minutes", 10);
        set_guests(&mut game, "hunger", 60);
        step_until(&mut game, 120, |now| {
            let ops = &food(now)["stand"];
            number(&ops["serving"]) > 0 && number(&ops["queue"]) > 0
        });
        game
    };

    // Save and reload in the middle of service: the park runs on exactly as if uninterrupted,
    // and every sale is counted once.
    let mut game = busy();
    let mut reloaded = ZooGame::new();
    accepted(reloaded.load_save(&game.save_json()));
    for _ in 0..120 {
        game.tick(1);
        reloaded.tick(1);
        assert_stands_consistent(&reloaded);
    }
    assert_eq!(game.save_json(), reloaded.save_json());
    let now = view(&game);
    let stand = food(&now);
    assert_eq!(
        number(&now["finance"]["current_day"]["breakdown"]["concession_income_cents"]),
        number(&stand["stand"]["today"]["revenue_cents"])
    );
    assert_eq!(
        number(&stand["stand"]["stock"]),
        number(&stand["stand"]["stock_capacity"]) - number(&stand["total_sales"])
    );

    // Demolition mid-service: guests leave without a sale, money or a meal; nothing is left
    // holding a place.
    let mut game = busy();
    let before = view(&game);
    let income = before["finance"]["current_day"]["breakdown"]["concession_income_cents"].clone();
    let hunger: Vec<(i64, i64)> = before["guests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|guest| (number(&guest["id"]), number(&guest["hunger"])))
        .collect();
    accepted(game.bulldoze(STAND.0, STAND.1));
    assert_stands_consistent(&game);
    let now = view(&game);
    assert_eq!(
        now["finance"]["current_day"]["breakdown"]["concession_income_cents"],
        income
    );
    assert_eq!(now["cash_cents"], before["cash_cents"]);
    for guest in now["guests"].as_array().unwrap() {
        assert!(guest["comfort"]["facility_id"].is_null(), "{guest}");
        let was = hunger
            .iter()
            .find(|(id, _)| *id == number(&guest["id"]))
            .unwrap()
            .1;
        assert_eq!(number(&guest["hunger"]), was);
    }
    game.tick(60);
    assert_stands_consistent(&game);

    // A path cut under guests walking to the stand: they route again or give up; nothing is
    // sold twice and no place is held by a guest who is not there.
    let mut game = hungry_park(150);
    set_guests(&mut game, "hunger", 60);
    step_until(&mut game, 60, |now| {
        now["guests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guest| guest["thought"] == "I'm off to get something to eat.")
    });
    accepted(game.bulldoze(11, 7));
    assert_stands_consistent(&game);
    game.tick(60);
    assert_stands_consistent(&game);
    let stand = food(&view(&game));
    assert_eq!(
        number(&stand["stand"]["total"]["served"]),
        number(&stand["total_sales"])
    );
    accepted(game.build_path_stroke(vec![11, 7], 1, "public".to_owned()));
    game.tick(60);
    assert_stands_consistent(&game);
}

#[test]
fn restocking_wages_and_sales_reconcile_with_the_ledger() {
    let mut game = park();
    accepted(game.place_concession(12, 6, "drink".to_owned()));
    accepted(game.hire_mechanic());
    accepted(game.set_stand_vendors(1, 2));
    // Low stock so automatic restocking orders during the run.
    set_stand_state(&mut game, "stock", 12.into());
    let catalog = json(&game.content_json());
    let wage = number(&catalog["balance"]["staff"]["vendor"]["hourly_wage_cents"]);
    let mut deliveries = 0;
    let mut stock_seen = Vec::new();
    for minute in 0..(2 * 24 * 60) {
        game.tick(1);
        if minute % 90 == 0 {
            set_guests(&mut game, "hunger", 55);
        }
        assert_eq!(game.session_status(), "running");
        let now = view(&game);
        let ledger = &now["finance"]["current_day"]["breakdown"];
        let stands: Vec<Value> = now["concessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|stand| stand["stand"].is_object())
            .cloned()
            .collect();
        let sum = |field: &str| -> i64 {
            stands
                .iter()
                .map(|stand| number(&stand["stand"]["today"][field]))
                .sum()
        };
        assert_eq!(
            sum("revenue_cents"),
            number(&ledger["concession_income_cents"])
        );
        assert_eq!(
            sum("stock_cost_cents"),
            number(&ledger["stand_stock_expense_cents"])
        );
        assert_eq!(
            sum("wage_cents"),
            number(&ledger["vendor_wages_expense_cents"])
        );
        for stand in &stands {
            let ops = &stand["stand"];
            let today = &ops["today"];
            assert_eq!(
                number(&today["contribution_cents"]),
                number(&today["revenue_cents"]) - number(&today["operating_cents"])
            );
            assert_eq!(number(&today["served"]), number(&stand["sales_today"]));
            assert!(number(&ops["stock"]) >= 0);
        }
        let food_stock = number(&food(&now)["stand"]["stock"]);
        if stock_seen.last().is_some_and(|last| food_stock > *last) {
            deliveries += 1;
        }
        stock_seen.push(food_stock);
        if now["minute_of_day"] == 30 {
            // One full hour of the previous day: wages are per vendor per hour.
            let hourly: i64 = stands
                .iter()
                .map(|stand| number(&stand["stand"]["vendors"]) * wage)
                .sum();
            assert_eq!(hourly, 3 * wage);
        }
    }
    assert!(deliveries > 0, "automatic restocking delivered");
    let total = &food(&view(&game))["stand"]["total"];
    assert!(number(&total["stock_cost_cents"]) > 0);
    assert!(number(&total["wage_cents"]) > 0);
    assert!(number(&total["upkeep_cents"]) > 0);
}

#[test]
fn more_vendors_raise_throughput_and_cut_lost_demand() {
    let run = |vendors: u32| {
        let mut game = hungry_park(240);
        add_guests(&mut game, 8);
        set_service(&mut game, "service_minutes", 8);
        accepted(game.set_stand_vendors(1, vendors));
        for _ in 0..6 {
            set_guests(&mut game, "hunger", 60);
            game.tick(30);
            assert_stands_consistent(&game);
        }
        let stand = food(&view(&game));
        (
            number(&stand["total_sales"]),
            number(&stand["stand"]["total"]["lost_sales"]),
            number(&stand["stand"]["capacity_per_hour"]),
        )
    };
    let (one_sales, one_lost, one_capacity) = run(1);
    let (three_sales, three_lost, three_capacity) = run(3);
    assert_eq!(three_capacity, 3 * one_capacity);
    assert!(three_sales > one_sales, "{three_sales} vs {one_sales}");
    assert!(three_lost < one_lost, "{three_lost} vs {one_lost}");
}

#[test]
fn queued_guests_cost_no_route_work_while_they_wait() {
    // Work per minute grows with sales, not with the queue: waiting guests never search.
    let mut game = hungry_park(240);
    add_guests(&mut game, 8);
    set_service(&mut game, "service_minutes", 10);
    set_service(&mut game, "queue_capacity", 12);
    set_guests(&mut game, "hunger", 60);
    step_until(&mut game, 120, |now| {
        number(&food(now)["stand"]["queue"]) >= 3
    });
    // Freeze everyone else: guests not at the stand stay where they are by viewing.
    let searches =
        |game: &ZooGame| number(&json(&game.diagnostics_json())["work"]["path_searches"]);
    let mut per_minute = Vec::new();
    for _ in 0..10 {
        let before = searches(&game);
        let queued = number(&food(&view(&game))["stand"]["queue"]);
        game.tick(1);
        per_minute.push((queued, searches(&game) - before));
    }
    let guests = view(&game)["guests"].as_array().unwrap().len() as i64;
    for (queued, work) in per_minute {
        assert!(
            work <= guests - queued + 2,
            "{work} searches with {queued} queued of {guests}"
        );
    }
}
