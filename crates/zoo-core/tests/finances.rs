//! Budgets, loans, financial history and fair insolvency recovery (#115), through the public
//! game seam: cash reconciliation across day boundaries, exploit-resistant borrowing and
//! repayment, a designed indebted park recovering through normal play, save/load across interest
//! and repayment boundaries, reports that separate operations from borrowing, budget plans and
//! the low-cash and deficit alerts.
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

fn refused(result: String) -> String {
    let result = json(&result);
    assert_eq!(result["ok"], false, "{result}");
    result["message"].as_str().unwrap().to_owned()
}

fn view(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

fn cents(value: &Value) -> i64 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("{value} is not whole cents"))
}

/// Advances `minutes`, in the player-facing steps of at most four hours.
fn run(game: &mut ZooGame, minutes: u32) {
    let mut left = minutes;
    while left > 0 {
        let step = left.min(240);
        game.tick(step);
        left -= step;
    }
}

/// Runs to the next midnight and stops at 00:00.
fn to_midnight(game: &mut ZooGame) {
    let minute = view(game)["minute_of_day"].as_u64().unwrap() as u32;
    run(game, 24 * 60 - minute);
}

/// The "Neglected corner zoo": $4,000 cash, $10,000 owed on a $15,000 credit line at 0.2 %
/// a day.
fn recovery() -> ZooGame {
    let mut game = ZooGame::new();
    accepted(game.start_scenario("recovery"));
    game
}

/// Two stocked habitats on one public path, staffed, with a food and a drink stand: a park with
/// income and every kind of recurring cost.
fn busy() -> ZooGame {
    let mut game = ZooGame::new();
    accepted(game.skip_tutorial());
    accepted(game.build_path_stroke(vec![1, 7, 14, 7], 1, "public".to_owned()));
    accepted(game.create_work_zone(0, 0, 19, 13));
    accepted(game.buy_animal_feed());
    for (index, (rect, species)) in [((3, 8, 5, 10), "capybara"), ((9, 8, 11, 10), "zebra")]
        .into_iter()
        .enumerate()
    {
        let id = index as u32 + 1;
        accepted(game.place_habitat_rect(rect.0, rect.1, rect.2, rect.3));
        accepted(game.hire_keeper());
        accepted(game.assign_keeper_zone(id, 1));
        accepted(game.adopt(id, species.to_owned()));
    }
    accepted(game.place_concession(7, 6, "food".to_owned()));
    accepted(game.place_concession(8, 6, "drink".to_owned()));
    accepted(game.hire_janitor());
    accepted(game.hire_mechanic());
    game
}

/// Every kept day, and today so far, explains its cash exactly, and the days chain.
fn assert_reconciles(game: &ZooGame) {
    let view = view(game);
    let finance = &view["finance"];
    let mut days: Vec<&Value> = finance["history"].as_array().unwrap().iter().collect();
    days.reverse();
    days.push(&finance["current_day"]);
    for day in &days {
        let breakdown = &day["breakdown"];
        let income = cents(&breakdown["admissions_income_cents"])
            + cents(&breakdown["concession_income_cents"]);
        let expenses: i64 = breakdown
            .as_object()
            .unwrap()
            .iter()
            .filter(|(field, _)| field.ends_with("_expense_cents"))
            .map(|(_, value)| cents(value))
            .sum();
        assert_eq!(cents(&day["income_cents"]), income, "day {}", day["day"]);
        assert_eq!(
            cents(&day["expenses_cents"]),
            expenses,
            "day {}",
            day["day"]
        );
        let flow = income - expenses + cents(&breakdown["loan_proceeds_cents"])
            - cents(&breakdown["loan_repayment_cents"]);
        assert_eq!(
            cents(&day["net_cash_flow_cents"]),
            flow,
            "day {}",
            day["day"]
        );
        assert_eq!(
            cents(&day["opening_cash_cents"]) + flow,
            cents(&day["closing_cash_cents"]),
            "day {} does not reconcile",
            day["day"]
        );
    }
    for pair in days.windows(2) {
        assert_eq!(
            pair[0]["day"].as_u64().unwrap() + 1,
            pair[1]["day"].as_u64().unwrap()
        );
        assert_eq!(pair[0]["closing_cash_cents"], pair[1]["opening_cash_cents"]);
    }
    assert_eq!(
        finance["current_day"]["closing_cash_cents"],
        view["cash_cents"]
    );
}

/// Cash reconciles with opening cash, operating flows, investment, loan proceeds and
/// repayments over several day boundaries; borrowing is never income and repaying never an
/// operating expense.
#[test]
fn cash_reconciles_across_day_boundaries_with_investment_and_loan_flows() {
    let mut game = busy();
    let before = view(&game)["finance"]["current_day"].clone();
    accepted(game.borrow_loan(500_000.0));
    let after = view(&game)["finance"]["current_day"].clone();
    for field in ["income_cents", "expenses_cents", "operating_result_cents"] {
        assert_eq!(after[field], before[field], "borrowing changed {field}");
    }
    assert_eq!(cents(&after["loan_proceeds_cents"]), 500_000);
    assert_eq!(cents(&after["net_borrowing_cents"]), 500_000);
    run(&mut game, 300);
    accepted(game.repay_loan(200_000.0));
    let repaid = view(&game)["finance"]["current_day"].clone();
    assert_eq!(cents(&repaid["loan_repayments_cents"]), 200_000);
    assert_eq!(
        cents(&repaid["operating_expenses_cents"]) + cents(&repaid["investment_cents"]),
        cents(&repaid["expenses_cents"]),
        "a repayment is not an expense"
    );
    to_midnight(&mut game);
    assert_reconciles(&game);
    // Day 2: an investment and more borrowing; day 3: a repayment, then into day 4.
    accepted(game.place_concession(10, 6, "drink".to_owned()));
    accepted(game.borrow_loan(100_000.0));
    run(&mut game, 1440);
    assert_reconciles(&game);
    accepted(game.repay_loan(150_000.0));
    run(&mut game, 1440 + 600);
    assert_reconciles(&game);
    let view = view(&game);
    let history = view["finance"]["history"].as_array().unwrap();
    assert_eq!(history.len(), 3);
    let total_flow: i64 = history
        .iter()
        .chain([&view["finance"]["current_day"]])
        .map(|day| cents(&day["net_cash_flow_cents"]))
        .sum();
    let opening = cents(&history.last().unwrap()["opening_cash_cents"]);
    assert_eq!(
        opening, 5_000_000,
        "day 1 opened with the scenario's starting cash"
    );
    assert_eq!(opening + total_flow, cents(&view["cash_cents"]));
    let loan = &view["finance"]["loan"];
    assert_eq!(cents(&loan["balance_cents"]), 250_000);
    assert_eq!(cents(&loan["borrowed_total_cents"]), 600_000);
    assert_eq!(cents(&loan["repaid_total_cents"]), 350_000);
    // The 7-day summary covers the three completed days and sums them.
    let week = &view["finance"]["periods"][0];
    assert_eq!(week["period_days"], 7);
    assert_eq!(week["days"], 3);
    assert_eq!(
        week["opening_cash_cents"],
        history.last().unwrap()["opening_cash_cents"]
    );
    assert_eq!(week["closing_cash_cents"], history[0]["closing_cash_cents"]);
    let summed: i64 = history
        .iter()
        .map(|day| cents(&day["net_cash_flow_cents"]))
        .sum();
    assert_eq!(cents(&week["flows"]["net_cash_flow_cents"]), summed);
    assert_eq!(cents(&week["flows"]["loan_proceeds_cents"]), 600_000);
    assert_eq!(cents(&week["flows"]["loan_repayments_cents"]), 350_000);
}

/// Repeated borrowing, repayment and cancellation cannot mint money, underflow what is owed,
/// exceed the limit or charge interest twice; invalid amounts change nothing.
#[test]
fn repeated_borrow_and_repay_commands_cannot_mint_money_or_duplicate_interest() {
    let mut game = recovery();
    let start = view(&game);
    let cash = cents(&start["cash_cents"]);
    assert_eq!(cents(&start["finance"]["loan"]["balance_cents"]), 1_000_000);
    assert_eq!(cents(&start["finance"]["loan"]["headroom_cents"]), 500_000);
    let unchanged = game.save_json();
    for amount in [f64::NAN, f64::INFINITY, -100.0, 0.5, 0.0, 500_001.0, 1e20] {
        refused(game.borrow_loan(amount));
        refused(game.repay_loan(amount));
    }
    refused(game.repay_loan(cash as f64 + 1.0));
    assert!(refused(game.repay_loan(1_000_001.0)).contains("owes only"));
    assert_eq!(
        json(&game.save_json())["park"],
        json(&unchanged)["park"],
        "refusals changed the park"
    );

    // Borrow and cancel five times in a day: cash returns exactly, the balance never drifts,
    // and only the day's highest balance draws interest, once.
    for _ in 0..5 {
        accepted(game.borrow_loan(400_000.0));
        refused(game.borrow_loan(100_001.0));
        accepted(game.repay_loan(400_000.0));
    }
    let after = view(&game);
    assert_eq!(cents(&after["cash_cents"]), cash);
    assert_eq!(cents(&after["finance"]["loan"]["balance_cents"]), 1_000_000);
    assert_eq!(
        cents(&after["finance"]["loan"]["interest_tonight_cents"]),
        2_800,
        "interest is due on the 1,400,000 peak at 0.2 %"
    );
    to_midnight(&mut game);
    assert_reconciles(&game);
    let closed = view(&game);
    let day = &closed["finance"]["history"][0];
    assert_eq!(cents(&day["interest_cents"]), 2_800);
    assert_eq!(cents(&day["loan_proceeds_cents"]), 2_000_000);
    assert_eq!(cents(&day["loan_repayments_cents"]), 2_000_000);
    assert_eq!(cents(&day["net_borrowing_cents"]), 0);
    assert_eq!(
        cents(&day["net_cash_flow_cents"]),
        cents(&day["operating_result_cents"]) - cents(&day["investment_cents"]) - 2_800,
        "borrowing and cancelling left only the day's interest"
    );
    // The next day draws interest on what is owed now, once, at its own close.
    let loan = &closed["finance"]["loan"];
    assert_eq!(cents(&loan["interest_total_cents"]), 2_800);
    assert_eq!(cents(&loan["interest_tonight_cents"]), 2_000);
    run(&mut game, 600);
    assert_eq!(
        cents(&view(&game)["finance"]["loan"]["interest_total_cents"]),
        2_800,
        "no interest before the next boundary"
    );
    to_midnight(&mut game);
    let loan = &view(&game)["finance"]["loan"];
    assert_eq!(cents(&loan["interest_total_cents"]), 4_800);
    assert_eq!(cents(&loan["balance_cents"]), 1_000_000);
    assert_reconciles(&game);
    // Repaying in full leaves nothing owed and nothing more to repay.
    let mut paid = busy();
    accepted(paid.borrow_loan(100_000.0));
    accepted(paid.repay_loan(100_000.0));
    assert_eq!(cents(&view(&paid)["finance"]["loan"]["balance_cents"]), 0);
    assert!(refused(paid.repay_loan(1.0)).contains("owes nothing"));
    // A park whose scenario offers no credit cannot borrow.
    let mut document = json(&busy().save_json());
    document["park"]["finance"]["loan"]["terms"] = Value::Null;
    let mut uncredited = ZooGame::new();
    accepted(uncredited.load_save(&document.to_string()));
    assert_eq!(view(&uncredited)["finance"]["loan"]["available"], false);
    assert!(refused(uncredited.borrow_loan(100.0)).contains("no loans"));
    // Nor can a save claim a debt beyond its limit.
    document["park"]["finance"]["loan"]["balance_cents"] = 1.into();
    assert_eq!(
        json(&uncredited.load_save(&document.to_string()))["ok"],
        false
    );
}

/// The park the browser proof opens (`apps/web/scripts/finances-proof.mjs`).
const PROOF_PARK: &str = "tests/fixtures/debt-park.json";

/// The designed indebted, loss-making park: the "Neglected corner zoo" borrowed another $1,000,
/// built an empty habitat, sold tickets at $4 and posted three vendors at its broken stand, then
/// ran into the red. It is saved at 20:00 on a day after one that closed in deficit.
fn indebted() -> ZooGame {
    let mut game = recovery();
    accepted(game.borrow_loan(100_000.0));
    accepted(game.place_habitat_rect(3, 7, 8, 10));
    accepted(game.set_admission_price(400.0));
    accepted(game.set_stand_vendors(1, 3));
    while cents(&view(&game)["finance"]["solvency"]["deficit_days"]) == 0 {
        to_midnight(&mut game);
    }
    run(&mut game, 20 * 60);
    game
}

/// The browser proof's park must stay a current, loadable save; rewrite it with
/// `ZOO_WRITE_DEBT_PARK=1`.
#[test]
fn the_debt_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_DEBT_PARK").is_some() {
        let value = json(&indebted().save_json());
        std::fs::write(
            PROOF_PARK,
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
    let fixture = std::fs::read_to_string(PROOF_PARK).unwrap();
    let mut game = ZooGame::new();
    accepted(game.load_save(&fixture));
    assert_eq!(json(&game.save_json()), json(&fixture));
    let view = view(&game);
    assert!(cents(&view["cash_cents"]) < 0, "the park is in deficit");
    assert_eq!(view["minute_of_day"], 20 * 60);
    assert!(cents(&view["finance"]["loan"]["headroom_cents"]) > 0);
}

/// A heavily indebted, loss-making park shows a understandable, costly recovery through normal
/// play: the deficit blocks spending without ending the game, the report names the causes and
/// the fixes, a bridge from the remaining credit and fair prices with fewer vendors turn
/// operations profitable, and every debt and cost stays on the books.
#[test]
fn an_indebted_park_recovers_through_normal_play_without_erasing_obligations() {
    let mut game = ZooGame::new();
    accepted(game.load_save(&std::fs::read_to_string(PROOF_PARK).unwrap()));
    let start = view(&game);
    let finance = &start["finance"];
    let solvency = &finance["solvency"];
    assert_eq!(solvency["status"], "deficit");
    assert_eq!(solvency["spending_blocked"], true);
    assert!(cents(&solvency["deficit_days"]) >= 1);
    let alert = start["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|alert| alert["code"] == "cash_deficit")
        .expect("a deficit alert");
    assert_eq!(alert["severity"], "critical");
    assert_eq!(alert["action"], "open_finances");
    assert_eq!(alert["subject"]["kind"], "finances");
    // The report says operations lose money and borrowing paid for part of the week.
    let week = &finance["periods"][0]["flows"];
    assert!(cents(&week["operating_result_cents"]) < 0);
    assert_eq!(week["verdict"], "borrowing_covers_losses");
    let drivers: Vec<&str> = finance["budget"]["drivers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["line"].as_str().unwrap())
        .collect();
    assert!(
        drivers.contains(&"vendor_wages_expense_cents"),
        "{drivers:?}"
    );
    let steps: Vec<&str> = solvency["recovery"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["kind"].as_str().unwrap())
        .collect();
    for step in [
        "raise_admission",
        "cut_vendors",
        "postpone_expansion",
        "borrow",
    ] {
        assert!(steps.contains(&step), "{step} missing from {steps:?}");
    }
    let fair = solvency["recovery"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["kind"] == "raise_admission")
        .unwrap()["fair_cents"]
        .clone();
    // In deficit, purchases wait; free decisions do not.
    assert!(refused(game.buy_animal_feed()).contains("Not enough cash"));
    let owed = cents(&finance["loan"]["balance_cents"]);
    let deficit = cents(&solvency["deficit_cents"]);
    let headroom = cents(&finance["loan"]["headroom_cents"]);
    // A bridge: the plan shows what it costs a day before the decision.
    let bridge = (deficit + 20_000).min(headroom);
    let plan = accepted(game.evaluate_budget(6_000.0, bridge as f64))["plan"].clone();
    assert_eq!(plan["within_limit"], true);
    assert_eq!(plan["affordable"], true);
    assert!(
        cents(&plan["interest_per_day_after_cents"])
            > cents(&finance["loan"]["interest_per_day_cents"])
    );
    assert_eq!(
        view(&game)["finance"]["loan"]["balance_cents"],
        owed,
        "a plan changes nothing"
    );
    refused(game.evaluate_budget(f64::NAN, 0.0));
    let over = accepted(game.evaluate_budget(0.0, (headroom + 1) as f64))["plan"].clone();
    assert_eq!(over["within_limit"], false);
    accepted(game.borrow_loan(bridge as f64));
    accepted(game.buy_animal_feed());
    accepted(game.set_admission_price(fair.as_f64().unwrap()));
    accepted(game.set_stand_vendors(1, 0));
    // Two days on, operations earn money, the deficit is gone and every obligation remains.
    to_midnight(&mut game);
    run(&mut game, 2 * 1440);
    let end = view(&game);
    let latest = &end["finance"]["history"][0];
    assert!(cents(&latest["operating_result_cents"]) > 0, "{latest}");
    assert!(cents(&latest["operating_result_cents"]) > cents(&latest["interest_cents"]));
    assert_eq!(latest["verdict"], "operations_profitable");
    assert!(cents(&end["cash_cents"]) > 0);
    assert_ne!(end["finance"]["solvency"]["status"], "deficit");
    assert!(
        !end["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alert| alert["code"] == "cash_deficit")
    );
    assert_eq!(
        cents(&end["finance"]["loan"]["balance_cents"]),
        owed + bridge
    );
    assert!(
        cents(&end["finance"]["loan"]["interest_total_cents"])
            > cents(&finance["loan"]["interest_total_cents"])
    );
    assert!(cents(&end["finance"]["solvency"]["deficit_days"]) >= cents(&solvency["deficit_days"]));
    assert_eq!(end["finance"]["solvency"]["deficit_streak_days"], 0);
    assert_eq!(game.session_status(), "running");
    assert_reconciles(&game);
}

/// Save/load just before an interest boundary and between repayments yields identical
/// obligations and outcomes, and the recorded session replays to the same park.
#[test]
fn save_and_load_across_interest_and_repayment_boundaries_keep_obligations() {
    let mut game = recovery();
    accepted(game.borrow_loan(300_000.0));
    run(&mut game, 120);
    accepted(game.repay_loan(250_000.0));
    let minute = view(&game)["minute_of_day"].as_u64().unwrap() as u32;
    run(&mut game, 24 * 60 - minute - 1);
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    accepted(loaded.load_save(&saved));
    assert_eq!(loaded.save_json(), saved);
    assert_eq!(
        view(&loaded)["finance"]["loan"]["interest_tonight_cents"],
        2_600,
        "tonight's interest is on the 1,300,000 peak"
    );
    for step in 0..6 {
        game.tick(1);
        loaded.tick(1);
        if step == 2 {
            accepted(game.repay_loan(10_000.0));
            accepted(loaded.repay_loan(10_000.0));
        }
        game.tick(239);
        loaded.tick(239);
        assert_eq!(game.save_json(), loaded.save_json());
    }
    let (a, b) = (view(&game), view(&loaded));
    assert_eq!(a["finance"]["loan"], b["finance"]["loan"]);
    assert_eq!(a["finance"]["history"], b["finance"]["history"]);
    assert_eq!(cents(&a["finance"]["loan"]["interest_total_cents"]), 2_600);
    assert_eq!(
        cents(&a["finance"]["loan"]["interest_tonight_cents"]),
        2_100
    );
    game.tick(1);
    loaded.tick(1);
    assert_eq!(game.save_json(), loaded.save_json());
    let a = view(&game);
    assert_eq!(
        cents(&a["finance"]["loan"]["interest_total_cents"]),
        2_600 + 2_100
    );
    assert_reconciles(&game);
    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json())
        .expect("the recorded session replays");
    assert_eq!(replayed.save_json(), game.save_json());
}

/// The report separates what operations earned from cash borrowing provided, per day and per
/// period, and the history keeps at most 30 days.
#[test]
fn reports_distinguish_profitable_operations_from_borrowed_cash() {
    let mut game = recovery();
    // Day 1: a fair price, and borrowing on top: operations earn and borrowing adds cash.
    accepted(game.set_admission_price(1_500.0));
    accepted(game.borrow_loan(200_000.0));
    to_midnight(&mut game);
    // Day 2: three vendors at the broken stand lose money; borrowing covers it.
    accepted(game.set_stand_vendors(1, 3));
    accepted(game.set_admission_price(100.0));
    accepted(game.borrow_loan(100_000.0));
    to_midnight(&mut game);
    // Day 3: still losing, nothing borrowed.
    to_midnight(&mut game);
    let view = view(&game);
    let history = view["finance"]["history"].as_array().unwrap();
    let (day3, day2, day1) = (&history[0], &history[1], &history[2]);
    assert_eq!(day1["verdict"], "operations_profitable");
    assert!(cents(&day1["operating_result_cents"]) > 0);
    assert_eq!(cents(&day1["net_borrowing_cents"]), 200_000);
    assert_eq!(day2["verdict"], "borrowing_covers_losses");
    assert!(cents(&day2["operating_result_cents"]) < 0);
    assert!(
        cents(&day2["net_cash_flow_cents"]) > 0,
        "borrowed cash, not profit"
    );
    assert_eq!(day3["verdict"], "operations_losing");
    assert!(cents(&day3["net_cash_flow_cents"]) < 0);
    let week = &view["finance"]["periods"][0]["flows"];
    assert_eq!(cents(&week["net_borrowing_cents"]), 300_000);
    assert_eq!(
        cents(&week["operating_result_cents"]),
        history
            .iter()
            .map(|day| cents(&day["operating_result_cents"]))
            .sum::<i64>()
    );

    // Thirty days at most are kept, newest first, and the 30-day period covers them.
    let mut long = recovery();
    for _ in 0..33 {
        long.tick(240);
        long.tick(240);
        long.tick(240);
        long.tick(240);
        long.tick(240);
        long.tick(240);
    }
    let view = json(&long.snapshot_json());
    let history = view["finance"]["history"].as_array().unwrap();
    assert_eq!(history.len(), 30);
    assert_eq!(history[0]["day"], 33);
    assert_eq!(history[29]["day"], 4);
    assert_eq!(view["finance"]["periods"][1]["days"], 30);
    assert_eq!(view["finance"]["periods"][0]["days"], 7);
    assert_reconciles(&long);
}

/// The budget lists today's recurring commitments and its assumptions, projects a runway only
/// while cash runs down, warns before cash runs out and counts cash net of debt for goals.
#[test]
fn budget_projection_states_assumptions_and_low_cash_warns_first() {
    let mut game = recovery();
    let start = view(&game);
    let budget = &start["finance"]["budget"];
    assert_eq!(budget["basis_days"], 0);
    let committed: i64 = budget["commitments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| cents(&line["cents"]))
        .sum();
    assert_eq!(
        cents(&budget["committed_per_day_cents"]),
        committed + cents(&start["finance"]["loan"]["interest_per_day_cents"])
    );
    assert!(
        budget["assumptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|text| text.as_str().unwrap().contains("not projected"))
    );
    to_midnight(&mut game);
    let budget = &view(&game)["finance"]["budget"];
    assert_eq!(budget["basis_days"], 1);
    assert!(cents(&budget["expected_income_per_day_cents"]) > 0);
    // A plan never changes the park.
    let before = game.save_json();
    accepted(game.evaluate_budget(100_000.0, 200_000.0));
    assert_eq!(game.save_json(), before);

    // Overstaff the broken stand until cash is short: a warning first, then the deficit.
    accepted(game.set_stand_vendors(1, 3));
    accepted(game.set_admission_price(0.0));
    let mut warned = false;
    while cents(&view(&game)["cash_cents"]) >= 0 {
        run(&mut game, 60);
        let view = view(&game);
        if view["finance"]["solvency"]["status"] == "low" {
            warned = true;
            assert!(
                view["alerts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|alert| alert["code"] == "low_cash")
            );
            assert!(view["finance"]["budget"]["runway_days"].is_i64());
        }
    }
    assert!(warned, "low cash warned before the deficit");
    let view = view(&game);
    assert!(
        view["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alert| alert["code"] == "cash_deficit")
    );
    assert!(
        !view["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alert| alert["code"] == "low_cash")
    );
    assert_eq!(view["finance"]["budget"]["runway_days"], 0);
}
