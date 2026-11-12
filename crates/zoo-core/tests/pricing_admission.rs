//! Player-controlled prices, the park gate and explainable demand (#106), through the public
//! game seam: authored low, fair and high price fixtures, exactly-once charging, budgets and
//! stock, closing, zero and invalid prices, save/load, replay, reachability and marketing.
use serde_json::Value;
use std::collections::HashSet;
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

/// Two stocked habitats on one public path from the entrance, staffed, with a food and a
/// drink stand between them. Stand 1 sells food and stand 2 drinks.
fn park() -> ZooGame {
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

fn view(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
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

fn number(value: &Value) -> i64 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

/// One day's results at a given admission price.
struct Run {
    at_gate: i64,
    admitted: i64,
    too_expensive: i64,
    admission: i64,
    /// Mean value perception of the guests inside at the end.
    average_value: i64,
}

fn run_admission(price: f64, minutes: u32) -> Run {
    let mut game = park();
    accepted(game.set_admission_price(price));
    run(&mut game, minutes);
    let view = view(&game);
    let today = &view["pricing"]["today"];
    assert_eq!(
        number(&today["admission_cents"]),
        number(&view["finance"]["current_day"]["breakdown"]["admissions_income_cents"]),
        "the gate and the ledger agree"
    );
    Run {
        at_gate: number(&today["people_at_gate"]),
        admitted: number(&today["people_admitted"]),
        too_expensive: number(&today["turned_away_too_expensive"]),
        admission: number(&today["admission_cents"]),
        average_value: view["pricing"]["average_value"].as_i64().unwrap_or(0),
    }
}

/// Low, fair and high admission prices over the same day of the same park: cheaper tickets
/// admit everyone, the default is accepted by every group (the forgiving default of Decision
/// 6.1.12), and a high price turns most groups away and earns less than the fair one, so
/// raising prices is never a guaranteed profit.
#[test]
fn low_fair_and_high_admission_prices_differ_intelligibly() {
    let low = run_admission(300.0, 600);
    let fair = run_admission(1_200.0, 600);
    let high = run_admission(3_000.0, 600);
    let extreme = run_admission(6_000.0, 600);
    for run in [&low, &fair] {
        assert!(run.at_gate > 0);
        assert_eq!(run.admitted, run.at_gate, "nobody refuses a fair ticket");
        assert_eq!(run.too_expensive, 0);
    }
    assert_eq!(low.admission, 300 * low.admitted);
    assert_eq!(fair.admission, 1_200 * fair.admitted);
    assert!(high.too_expensive > 0, "a high price turns groups away");
    assert!(high.admitted < fair.admitted);
    assert!(
        high.admission < fair.admission,
        "a high price earned more admission ({}) than the fair one ({})",
        high.admission,
        fair.admission
    );
    assert_eq!(extreme.admitted, 0, "nobody pays far over the park's value");
    assert_eq!(extreme.admission, 0);
    // The few who pay a premium think less of their visit than guests at a fair price.
    assert!(
        high.average_value < fair.average_value && fair.average_value < low.average_value,
        "{} / {} / {}",
        low.average_value,
        fair.average_value,
        high.average_value
    );
}

/// Low, fair and high food prices at the same park: sales fall as the price rises, guests
/// complain once it is over what they think fair, and a price far above the reference sells
/// nothing at all.
#[test]
fn low_fair_and_high_stand_prices_differ_intelligibly() {
    let sales = |price: f64| {
        let mut game = park();
        accepted(game.set_stand_price(1, price));
        run(&mut game, 600);
        let view = view(&game);
        let stand = &view["pricing"]["stands"][0];
        assert_eq!(stand["kind"], "food");
        (
            number(&stand["served_today"]),
            number(&stand["revenue_today_cents"]),
            number(&stand["too_expensive_today"]),
        )
    };
    let (low, low_revenue, low_complaints) = sales(250.0);
    let (fair, fair_revenue, fair_complaints) = sales(500.0);
    let (high, _, high_complaints) = sales(700.0);
    let (extreme, extreme_revenue, extreme_complaints) = sales(1_000.0);
    assert!(fair > 0, "the reference price sells");
    assert!(low >= fair && fair > high, "{low} / {fair} / {high}");
    assert_eq!(low_revenue, 250 * low);
    assert_eq!(fair_revenue, 500 * fair);
    assert_eq!((low_complaints, fair_complaints), (0, 0));
    assert!(high_complaints > 0, "guests complain about a dear stand");
    assert_eq!(extreme, 0);
    assert_eq!(extreme_revenue, 0);
    assert!(extreme_complaints > high_complaints);
    assert!(extreme_revenue < fair_revenue);
}

/// Groups that will not pay are turned away whole and pay nothing; admitted people pay once,
/// out of their seeded budget, and are never charged admission again.
#[test]
fn rejected_arrivals_pay_nothing_and_admitted_people_pay_once() {
    let mut game = park();
    accepted(game.set_admission_price(6_000.0));
    run(&mut game, 300);
    let view_closed = view(&game);
    let today = &view_closed["pricing"]["today"];
    assert!(number(&today["groups_at_gate"]) > 0);
    assert_eq!(number(&today["people_admitted"]), 0);
    assert_eq!(number(&today["admission_cents"]), 0);
    assert_eq!(
        number(&view_closed["finance"]["current_day"]["breakdown"]["admissions_income_cents"]),
        0
    );
    assert_eq!(
        number(&view_closed["guest_count"]),
        0,
        "nobody turned away entered"
    );
    let recent = view_closed["pricing"]["recent"].as_array().unwrap();
    assert!(
        recent.iter().all(
            |note| note["outcome"] == "too_expensive" && number(&note["willing_cents"]) < 6_000
        )
    );

    // Now a fair price: each new guest holds a budget step less the admission it paid.
    accepted(game.set_admission_price(1_000.0));
    let budgets: HashSet<i64> = (0..5).map(|step| 2_000 + 400 * step).collect();
    let mut seen = HashSet::new();
    for _ in 0..300 {
        game.tick(1);
        for guest in view(&game)["guests"].as_array().unwrap() {
            let id = number(&guest["id"]);
            if seen.insert(id) {
                let paid = number(&guest["cash_cents"]) + 1_000;
                assert!(budgets.contains(&paid), "guest {id} arrived with {paid}");
                assert!(guest["state"] == "arriving" || guest["state"] == "walking_to_habitat");
            }
        }
    }
    let view = view(&game);
    let admitted = number(&view["pricing"]["today"]["people_admitted"]);
    assert_eq!(
        admitted,
        seen.len() as i64,
        "every admitted person is one guest"
    );
    assert_eq!(
        number(&view["finance"]["current_day"]["breakdown"]["admissions_income_cents"]),
        1_000 * admitted,
        "each admitted person paid admission exactly once"
    );
}

/// Shop purchases never overspend a guest or empty a stand below zero, and every ledger
/// reconciles: the park's concession income is the stands' revenue, and the gate's admission
/// is the park's admission income, after a day with tight budgets and a dear drink.
#[test]
fn purchases_stay_within_budgets_and_stock_and_ledgers_reconcile() {
    let mut game = park();
    // A dear ticket leaves the least-funded guests little spending money.
    accepted(game.set_admission_price(1_900.0));
    accepted(game.set_stand_price(2, 450.0));
    for _ in 0..60 {
        game.tick(10);
        let view = view(&game);
        for guest in view["guests"].as_array().unwrap() {
            assert!(number(&guest["cash_cents"]) >= 0, "{guest}");
        }
        for stand in view["concessions"].as_array().unwrap() {
            if let Some(ops) = stand["stand"].as_object() {
                assert!(number(&ops["stock"]) >= 0);
            }
        }
        let income = &view["finance"]["current_day"]["breakdown"];
        let stand_revenue: i64 = view["concessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|stand| stand["stand"]["today"]["revenue_cents"].as_i64())
            .sum();
        assert_eq!(number(&income["concession_income_cents"]), stand_revenue);
        assert_eq!(
            number(&income["admissions_income_cents"]),
            number(&view["pricing"]["today"]["admission_cents"])
        );
        assert_eq!(game.session_status(), "running");
    }
    let view = view(&game);
    let unaffordable: i64 = view["pricing"]["stands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stand| number(&stand["unaffordable_today"]))
        .sum();
    assert!(unaffordable > 0, "some guests could not afford the stands");
}

/// A guest at a stand's window pays the price it agreed there, even when the player raises
/// the price before the sale completes.
#[test]
fn a_price_change_never_alters_an_agreed_sale() {
    let mut game = park();
    let mut agreed = Vec::new();
    for _ in 0..600 {
        game.tick(1);
        let save = json(&game.save_json());
        agreed = save["park"]["guests"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|guest| {
                guest["state"] == "using_facility" && guest["visit"]["facility_id"] == 1
            })
            .map(|guest| (number(&guest["id"]), number(&guest["visit"]["price_cents"])))
            .collect();
        if !agreed.is_empty() {
            break;
        }
    }
    assert!(!agreed.is_empty(), "nobody was served at the food stand");
    assert!(agreed.iter().all(|(_, price)| *price == 500));
    let before = view(&game);
    let food = |view: &Value| view["concessions"][0]["stand"]["total"].clone();
    // Far over anybody's tolerance: nobody new agrees, so only the agreed sales complete.
    accepted(game.set_stand_price(1, 2_000.0));
    game.tick(20);
    let after = view(&game);
    let served = number(&food(&after)["served"]) - number(&food(&before)["served"]);
    let revenue = number(&food(&after)["revenue_cents"]) - number(&food(&before)["revenue_cents"]);
    assert_eq!(served, agreed.len() as i64);
    assert_eq!(revenue, agreed.iter().map(|(_, price)| price).sum::<i64>());
}

/// Closing turns every arriving group away without charging it while guests inside finish
/// their visit; reopening admits again. Zero prices admit and sell for free with no income.
#[test]
fn closing_reopening_and_zero_prices_are_explicit() {
    let mut game = park();
    run(&mut game, 120);
    let inside = number(&view(&game)["guest_count"]);
    assert!(inside > 0);
    accepted(game.set_park_open(false));
    assert!(
        accepted(game.set_park_open(false))["message"]
            .as_str()
            .unwrap()
            .contains("already closed")
    );
    let before = view(&game)["pricing"]["today"].clone();
    run(&mut game, 120);
    let closed = view(&game);
    let today = &closed["pricing"]["today"];
    assert_eq!(today["people_admitted"], before["people_admitted"]);
    assert_eq!(today["admission_cents"], before["admission_cents"]);
    assert!(number(&today["turned_away_closed"]) > 0);
    assert_eq!(closed["pricing"]["open"], false);
    accepted(game.set_park_open(true));
    run(&mut game, 120);
    let reopened = view(&game);
    assert!(
        number(&reopened["pricing"]["today"]["people_admitted"])
            > number(&today["people_admitted"])
    );

    // Free admission and a free drink.
    let mut free = park();
    accepted(free.set_admission_price(0.0));
    accepted(free.set_stand_price(2, 0.0));
    run(&mut free, 480);
    let view = view(&free);
    assert!(number(&view["pricing"]["today"]["people_admitted"]) > 0);
    assert_eq!(number(&view["pricing"]["today"]["admission_cents"]), 0);
    let breakdown = &view["finance"]["current_day"]["breakdown"];
    assert_eq!(number(&breakdown["admissions_income_cents"]), 0);
    let drink = &view["pricing"]["stands"][1];
    assert!(
        number(&drink["served_today"]) > 0,
        "a free drink is still served"
    );
    assert_eq!(number(&drink["revenue_today_cents"]), 0);
}

/// Every malformed price is refused with a reason and changes nothing.
#[test]
fn invalid_prices_are_refused_and_change_nothing() {
    let mut game = park();
    let before = game.save_json();
    for bad in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        0.5,
        10_001.0,
        1e20,
    ] {
        let message = refused(game.set_admission_price(bad));
        assert!(message.starts_with("Admission must be"), "{message}");
        refused(game.set_stand_price(1, bad));
        assert_eq!(json(&game.evaluate_admission_price(bad))["ok"], false);
    }
    refused(game.set_stand_price(1, 5_001.0));
    assert!(refused(game.set_stand_price(99, 100.0)).contains("stand"));
    // A toilet has no price to set.
    accepted(game.place_concession(10, 6, "toilet".to_owned()));
    let after_toilet = game.save_json();
    refused(game.set_stand_price(3, 100.0));
    assert_eq!(game.save_json(), after_toilet);
    let mut original = json(&before);
    let mut now = json(&after_toilet);
    // Only the toilet differs.
    for value in [&mut original, &mut now] {
        let park = value["park"].as_object_mut().unwrap();
        for key in [
            "concessions",
            "tiles",
            "cash_cents",
            "finance_today",
            "next_concession_id",
            "alerts",
        ] {
            park.remove(key);
        }
    }
    assert_eq!(original, now);
    assert_eq!(view(&game)["pricing"]["admission_price_cents"], 1_200);
}

/// Prices, the gate record and a campaign survive save/load and replay, and the park runs on
/// exactly like one that never stopped.
#[test]
fn prices_survive_save_load_and_replay() {
    let mut game = park();
    accepted(game.set_admission_price(1_800.0));
    accepted(game.set_stand_price(1, 600.0));
    accepted(game.start_marketing_campaign());
    run(&mut game, 200);
    accepted(game.set_park_open(false));
    game.tick(30);
    accepted(game.set_park_open(true));
    refused(game.set_admission_price(f64::NAN));
    game.tick(30);
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    accepted(loaded.load_save(&saved));
    assert_eq!(loaded.save_json(), saved);
    for _ in 0..6 {
        game.tick(30);
        loaded.tick(30);
        assert_eq!(game.save_json(), loaded.save_json());
    }
    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json())
        .expect("the recorded session replays");
    assert_eq!(replayed.save_json(), game.save_json());
    let view = view(&loaded);
    assert_eq!(view["pricing"]["admission_price_cents"], 1_800);
    assert_eq!(view["pricing"]["stands"][0]["price_cents"], 600);
    assert!(number(&view["pricing"]["total"]["groups_at_gate"]) > 0);
}

/// A habitat nobody can reach adds nothing to what the park is worth and cannot stop groups
/// from coming to the habitats they can reach (#103's reachability fix stays effective).
#[test]
fn unreachable_habitats_neither_add_value_nor_block_admission() {
    let reachable = || {
        let mut game = ZooGame::new();
        accepted(game.skip_tutorial());
        accepted(game.build_path_stroke(vec![1, 7, 8, 7], 1, "public".to_owned()));
        accepted(game.create_work_zone(0, 0, 19, 13));
        accepted(game.buy_animal_feed());
        accepted(game.place_habitat_rect(3, 8, 5, 10));
        accepted(game.hire_keeper());
        accepted(game.assign_keeper_zone(1, 1));
        accepted(game.adopt(1, "capybara".to_owned()));
        game
    };
    let mut plain = reachable();
    let mut isolated = reachable();
    // A stocked habitat beside a path that does not connect to the entrance.
    accepted(isolated.build_path_stroke(vec![12, 1, 18, 1], 1, "public".to_owned()));
    accepted(isolated.place_habitat_rect(13, 2, 15, 4));
    accepted(isolated.hire_keeper());
    accepted(isolated.assign_keeper_zone(2, 1));
    accepted(isolated.adopt(2, "zebra".to_owned()));
    run(&mut plain, 360);
    run(&mut isolated, 360);
    let (plain, isolated) = (view(&plain), view(&isolated));
    assert_eq!(isolated["pricing"]["valued_habitats"], 1);
    assert_eq!(
        isolated["pricing"]["experience_value_cents"],
        plain["pricing"]["experience_value_cents"]
    );
    assert!(number(&isolated["pricing"]["today"]["people_admitted"]) > 0);
    assert_eq!(
        isolated["pricing"]["today"]["people_admitted"],
        plain["pricing"]["today"]["people_admitted"]
    );
}

/// The forecast replays the latest groups at the gate against a candidate price and says so;
/// it is monotone in the price and changes nothing.
#[test]
fn the_admission_forecast_states_its_assumptions() {
    let mut game = park();
    let empty = json(&game.evaluate_admission_price(1_200.0));
    assert_eq!(empty["forecast"]["sample_groups"], 0);
    assert!(
        empty["forecast"]["assumptions"]
            .as_str()
            .unwrap()
            .contains("nothing to forecast")
    );
    run(&mut game, 480);
    let saved = game.save_json();
    let at = |price: f64| json(&game.evaluate_admission_price(price))["forecast"].clone();
    let free = at(0.0);
    let sample = number(&free["sample_people"]);
    assert!(sample > 0);
    assert_eq!(number(&free["accepting_people"]), sample);
    let mut previous = sample;
    for price in [1_000.0, 1_500.0, 2_000.0, 2_500.0, 3_000.0, 10_000.0] {
        let forecast = at(price);
        let accepting = number(&forecast["accepting_people"]);
        assert!(accepting <= previous);
        assert_eq!(
            number(&forecast["admission_cents"]),
            price as i64 * accepting
        );
        assert!(
            forecast["assumptions"]
                .as_str()
                .unwrap()
                .contains("Assumes the same visitors")
        );
        previous = accepting;
    }
    assert_eq!(previous, 0);
    assert_eq!(game.save_json(), saved, "forecasting changes nothing");
}

/// A campaign costs its price once, brings groups to the gate more often for one day only,
/// and cannot be stacked or bought without the cash.
#[test]
fn marketing_is_bounded_explicit_spend_with_a_temporary_effect() {
    let mut plain = park();
    let mut marketed = park();
    let cash = number(&view(&marketed)["cash_cents"]);
    let started = accepted(marketed.start_marketing_campaign());
    assert!(started["message"].as_str().unwrap().contains("$250"));
    let spent = view(&marketed);
    assert_eq!(number(&spent["cash_cents"]), cash - 25_000);
    assert_eq!(
        number(&spent["finance"]["current_day"]["breakdown"]["marketing_expense_cents"]),
        25_000
    );
    assert!(refused(marketed.start_marketing_campaign()).contains("already running"));
    run(&mut plain, 600);
    run(&mut marketed, 600);
    let gate = |game: &ZooGame| number(&view(game)["pricing"]["total"]["groups_at_gate"]);
    let (plain_groups, marketed_groups) = (gate(&plain), gate(&marketed));
    assert!(
        marketed_groups > plain_groups,
        "{marketed_groups} groups with a campaign, {plain_groups} without"
    );
    assert!(view(&marketed)["pricing"]["campaign"].is_object());
    // A day later the campaign is over and arrivals return to the ordinary pace.
    run(&mut marketed, 900);
    run(&mut plain, 900);
    assert!(view(&marketed)["pricing"]["campaign"].is_null());
    let (plain_before, marketed_before) = (gate(&plain), gate(&marketed));
    run(&mut plain, 480);
    run(&mut marketed, 480);
    let plain_pace = gate(&plain) - plain_before;
    let marketed_pace = gate(&marketed) - marketed_before;
    assert!(
        marketed_pace.abs_diff(plain_pace) <= 2,
        "{marketed_pace} vs {plain_pace} groups after the campaign"
    );
    accepted(marketed.start_marketing_campaign());
}

const PROOF_PARK: &str = "tests/fixtures/pricing-park.json";

/// The browser proof's park (`apps/web/scripts/pricing-proof.mjs`): the two-habitat park two
/// hours into the day with an overpriced food stand turning guests away. It must stay a
/// current, loadable save; rewrite it with `ZOO_WRITE_PRICING_PARK=1`.
#[test]
fn the_pricing_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_PRICING_PARK").is_some() {
        let mut game = park();
        accepted(game.set_stand_price(1, 800.0));
        run(&mut game, 120);
        let value = json(&game.save_json());
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
    assert_eq!(view["pricing"]["stands"][0]["price_cents"], 800);
    assert!(number(&view["guest_count"]) > 0);
    run(&mut game, 240);
    assert_eq!(game.session_status(), "running");
}
