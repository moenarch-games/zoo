//! The park calendar, daylight, scenario weather and bounded park events (#118), through the
//! public game seam: the same seed, calendar and commands give the same weather and event
//! timing across save/load and however time is advanced; a warned hot day can be prepared for
//! and recovered from; events neither queue up nor repeat inside their cooldown and a missed
//! one costs nothing; switching a rule off changes outcomes; the browser proof's park stays a
//! current save.
use serde_json::Value;
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) -> Value {
    let result = json(&result);
    assert_eq!(result["ok"], true, "{result}");
    result
}

fn view(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

fn conditions(game: &ZooGame) -> Value {
    view(game)["conditions"].clone()
}

fn day(game: &ZooGame) -> u64 {
    view(game)["day"].as_u64().unwrap()
}

fn minute(game: &ZooGame) -> u64 {
    view(game)["minute_of_day"].as_u64().unwrap()
}

/// Ticks to `minute` of `day` (the park must be earlier).
fn run_to(game: &mut ZooGame, target_day: u64, target_minute: u64) {
    let now = (day(game) - 1) * 1440 + minute(game);
    let target = (target_day - 1) * 1440 + target_minute;
    assert!(target >= now, "the park is already past day {target_day}");
    let mut left = target - now;
    while left > 0 {
        let step = left.min(240);
        game.tick(step as u32);
        left -= step;
    }
}

/// The lived-in starter park on its scenario weather: spring for days 1–4, then summer, whose
/// first hot days are 5 and 6; a class of eight is announced for day 2 at 10:00, and the next
/// visit waits out a four-day cooldown.
fn living() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.start_scenario("living-starter"));
    game
}

/// The welfare part `key` of habitat `id`, if it has one.
fn part(game: &ZooGame, id: u64, key: &str) -> Option<Value> {
    view(game)["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|habitat| habitat["id"] == id)
        .unwrap()["welfare_parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["key"] == key)
        .cloned()
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

fn school_guests(game: &ZooGame) -> usize {
    view(game)["guests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|guest| guest["group"]["profile"] == "school")
        .count()
}

fn drinks_sold_today(game: &ZooGame) -> u64 {
    view(game)["concessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|stand| stand["kind"] == "drink")
        .map(|stand| stand["sales_today"].as_u64().unwrap())
        .sum()
}

#[test]
fn the_calendar_forecast_and_events_follow_the_scenario() {
    let mut game = living();
    let start = conditions(&game);
    assert_eq!(start["season"], "spring");
    assert_eq!(start["season_day"], 1);
    assert_eq!(start["weather_stream"], true);
    assert_eq!(start["light"]["phase"], "day");
    let forecast: Vec<_> = start["forecast"]
        .as_array()
        .unwrap()
        .iter()
        .map(|day| (day["day"].as_u64().unwrap(), day["weather"].clone()))
        .collect();
    assert_eq!(
        forecast,
        [(1, "fair".into()), (2, "fair".into()), (3, "fair".into())]
    );
    // A day ahead: the class is announced for tomorrow at 10:00.
    let scheduled = &start["scheduled"];
    assert_eq!(
        (
            &scheduled["day"],
            &scheduled["minute"],
            &scheduled["guests"]
        ),
        (&2.into(), &600.into(), &8.into())
    );
    assert!(
        scheduled["message"]
            .as_str()
            .unwrap()
            .starts_with("Tomorrow at 10:00: a class of 8 students arrives")
    );
    // Night falls and the sun rises again by the simulation clock.
    run_to(&mut game, 1, 23 * 60);
    assert_eq!(conditions(&game)["light"]["phase"], "night");
    assert_eq!(conditions(&game)["light"]["daylight_percent"], 0);
    run_to(&mut game, 2, 9 * 60 + 59);
    assert_eq!(school_guests(&game), 0);
    game.tick(1);
    assert_eq!(school_guests(&game), 8, "the class arrives at 10:00");
    let after = conditions(&game);
    assert!(after["scheduled"].is_null());
    assert_eq!(after["history"][0]["outcome"], "arrived");
    assert_eq!(after["history"][0]["day"], 2);
    // Summer brings the first hot days, forecast from day 3.
    run_to(&mut game, 3, 12 * 60);
    let forecast = conditions(&game)["forecast"].clone();
    assert_eq!(forecast[2]["day"], 5);
    assert_eq!(forecast[2]["weather"], "heat");
    assert_eq!(forecast[2]["season"], "summer");
}

/// Hourly snapshots of the calendar, its events and the park's guests.
fn timeline(game: &mut ZooGame, days: u64, step: impl Fn(&mut ZooGame, u64)) -> Vec<Value> {
    let mut seen = Vec::new();
    for slot in 0..days * 24 {
        step(game, slot);
        let view = view(game);
        let conditions = &view["conditions"];
        seen.push(serde_json::json!({
            "day": view["day"],
            "minute": view["minute_of_day"],
            "weather": conditions["weather"],
            "scheduled": conditions["scheduled"],
            "history": conditions["history"],
            "guests": view["guest_count"],
        }));
    }
    seen
}

#[test]
fn identical_seeds_and_commands_give_identical_conditions_across_save_and_load() {
    let uninterrupted = timeline(&mut living(), 7, |game, _| game.tick(60));
    // The same park saved and reloaded every seven hours.
    let reloaded = timeline(&mut living(), 7, |game, slot| {
        game.tick(60);
        if slot % 7 == 6 {
            let save = game.save_json();
            ok(game.load_save(&save));
        }
    });
    assert_eq!(uninterrupted, reloaded);
    // Two visits four days apart, and the hot days of the forecast.
    let last = uninterrupted.last().unwrap();
    let days: Vec<_> = last["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["day"].as_u64().unwrap())
        .collect();
    assert_eq!(days, [6, 2], "newest first");
    let mut hot: Vec<_> = uninterrupted
        .iter()
        .filter(|entry| entry["weather"] == "heat")
        .map(|entry| entry["day"].as_u64().unwrap())
        .collect();
    hot.dedup();
    assert_eq!(hot, [5, 6, 8]);
}

#[test]
fn pausing_speed_and_chunked_advances_cannot_skip_or_duplicate_events() {
    let target = 3 * 1440;
    let mut minute_by_minute = living();
    for _ in 0..target {
        minute_by_minute.tick(1);
    }
    let mut fast = living();
    // The browser clamps one advance to 240 minutes; odd chunks cross every boundary.
    for chunk in [7_u32, 240, 13, 240, 240, 1] {
        fast.tick(chunk);
    }
    let elapsed = |game: &ZooGame| (day(game) - 1) * 1440 + minute(game) - 9 * 60;
    run_to(
        &mut fast,
        1 + (9 * 60 + target) / 1440,
        (9 * 60 + target) % 1440,
    );
    // Pausing advances nothing.
    let paused = fast.save_json();
    fast.tick(0);
    assert_eq!(fast.save_json(), paused);
    assert_eq!(elapsed(&fast), elapsed(&minute_by_minute));
    assert_eq!(fast.save_json(), minute_by_minute.save_json());
    let history = conditions(&fast)["history"].as_array().unwrap().len();
    assert_eq!(history, 1, "the day-2 visit happened exactly once");
}

#[test]
fn a_hot_day_is_warned_a_day_ahead_and_shelter_prepares_and_recovers() {
    let mut game = living();
    run_to(&mut game, 4, 8 * 60);
    assert_eq!(conditions(&game)["forecast"][1]["weather"], "heat");
    // The warning: each stocked habitat without shade for every animal, with its fix.
    let warnings = alerts(&game, "heat_shade");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    for warning in &warnings {
        assert_eq!(warning["severity"], "notice");
        assert_eq!(warning["action"], "add_shelter");
        assert!(
            warning["message"]
                .as_str()
                .unwrap()
                .starts_with("Tomorrow will be hot")
        );
    }
    // Nothing is hot yet: no heat part on a fair day.
    assert!(part(&game, 1, "heat").is_none());
    // Prepare habitat #1 (four capybaras): one shelter shades all four.
    ok(game.add_shelter(1));
    let warnings = alerts(&game, "heat_shade");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["subject"]["id"], 2);

    run_to(&mut game, 5, 12 * 60);
    assert_eq!(conditions(&game)["weather"], "heat");
    let prepared = part(&game, 1, "heat").expect("a hot day has a heat part");
    assert_eq!(
        (&prepared["score"], &prepared["met"]),
        (&100.into(), &true.into())
    );
    let exposed = part(&game, 2, "heat").unwrap();
    assert_eq!(exposed["score"], 60, "no shade at all keeps the floor");
    assert_eq!(
        exposed["detail"],
        "Hot day: shade or cooling for 0 of 3 animals"
    );
    let warning = &alerts(&game, "heat_shade")[0];
    assert_eq!(warning["severity"], "warning");
    // Recovery: a shelter during the heat restores comfort at once.
    ok(game.add_shelter(2));
    assert_eq!(part(&game, 2, "heat").unwrap()["score"], 100);
    assert!(alerts(&game, "heat_shade").is_empty());
}

#[test]
fn heat_raises_drink_demand_and_switching_weather_effects_off_removes_every_effect() {
    let mut hot = living();
    run_to(&mut hot, 4, 23 * 60);
    let mut mild = living();
    run_to(&mut mild, 4, 23 * 60);
    ok(mild.set_condition_rule("weather_effects".to_owned(), false));
    assert!(alerts(&mild, "heat_shade").is_empty());
    run_to(&mut hot, 5, 23 * 60);
    run_to(&mut mild, 5, 23 * 60);
    let off = conditions(&mild);
    assert_eq!(off["weather"], "heat", "the forecast still shows the heat");
    assert_eq!(off["effects_active"], false);
    assert!(part(&mild, 2, "heat").is_none());
    assert!(part(&hot, 2, "heat").is_some());
    assert!(
        drinks_sold_today(&hot) > drinks_sold_today(&mild),
        "hot guests buy more drinks: {} vs {}",
        drinks_sold_today(&hot),
        drinks_sold_today(&mild)
    );
}

#[test]
fn events_never_queue_up_respect_their_cooldown_and_a_missed_one_costs_nothing() {
    let mut game = living();
    // The gate is closed when the class comes: the visit is missed, nobody pays or enters.
    ok(game.set_park_open(false));
    run_to(&mut game, 2, 9 * 60 + 59);
    let cash = view(&game)["cash_cents"].as_i64().unwrap();
    let ledger = view(&game)["finance"].clone();
    game.tick(1);
    assert_eq!(school_guests(&game), 0);
    assert_eq!(conditions(&game)["history"][0]["outcome"], "park_closed");
    assert!(view(&game)["cash_cents"].as_i64().unwrap() <= cash);
    assert_eq!(
        view(&game)["finance"]["today"]["admissions_cents"],
        ledger["today"]["admissions_cents"]
    );
    ok(game.set_park_open(true));
    // At most one announcement at any time, at most one event a day, cooldowns kept.
    let mut announced = Vec::new();
    for _ in 0..(12 * 24) {
        game.tick(60);
        let conditions = conditions(&game);
        if let Some(day) = conditions["scheduled"]["day"].as_u64() {
            announced.push(day);
        }
        assert!(conditions["history"].as_array().unwrap().len() <= 8);
    }
    let history = conditions(&game)["history"].clone();
    let mut days: Vec<_> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["day"].as_u64().unwrap())
        .collect();
    days.reverse();
    assert!(days.len() >= 3, "{days:?}");
    for pair in days.windows(2) {
        assert!(
            pair[1] >= pair[0] + 4,
            "a visit inside the cooldown: {days:?}"
        );
    }
    announced.dedup();
    assert!(announced.iter().all(|day| days.contains(day)));
}

#[test]
fn switching_events_off_cancels_the_announcement_and_stops_new_ones() {
    let mut game = living();
    assert!(conditions(&game)["scheduled"].is_object());
    ok(game.set_condition_rule("events".to_owned(), false));
    let off = conditions(&game);
    assert!(off["scheduled"].is_null());
    assert_eq!(off["history"][0]["outcome"], "cancelled");
    assert_eq!(off["rules"]["events"], false);
    let unknown = json(&game.set_condition_rule("tornadoes".to_owned(), true));
    assert_eq!(unknown["ok"], false);
    run_to(&mut game, 8, 12 * 60);
    assert!(conditions(&game)["scheduled"].is_null());
    assert_eq!(conditions(&game)["history"].as_array().unwrap().len(), 1);
    assert_eq!(school_guests(&game), 0);
    // Switched on again, announcements resume from the next day not yet considered.
    ok(game.set_condition_rule("events".to_owned(), true));
    run_to(&mut game, 10, 12 * 60);
    assert!(conditions(&game)["history"].as_array().unwrap().len() > 1);
    // The rule switches are recorded commands: the session replays to the same park.
    let bundle = game.diagnostics_json();
    let replayed = ZooGame::replay(&game.content_json(), &bundle).unwrap();
    assert_eq!(replayed.save_json(), game.save_json());
}

const PROOF_PARK: &str = "tests/fixtures/conditions-park.json";

/// The park the browser proof (apps/web/scripts/conditions-proof.mjs) opens: the lived-in
/// starter on day 4 at 20:00, the evening before its first hot day, with no shelter in either
/// habitat. It must stay a current, loadable save; rewrite it with `ZOO_WRITE_CONDITIONS_PARK=1`.
#[test]
fn the_conditions_proof_park_is_a_current_save() {
    if std::env::var_os("ZOO_WRITE_CONDITIONS_PARK").is_some() {
        let mut game = living();
        run_to(&mut game, 4, 20 * 60);
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
    // The proof's journey: tomorrow is hot, both habitats are warned, shelter fixes one.
    assert_eq!(conditions(&game)["forecast"][1]["weather"], "heat");
    assert_eq!(conditions(&game)["light"]["phase"], "dusk");
    assert_eq!(alerts(&game, "heat_shade").len(), 2);
    ok(game.add_shelter(1));
    run_to(&mut game, 5, 9 * 60);
    assert_eq!(part(&game, 1, "heat").unwrap()["score"], 100);
    assert_eq!(part(&game, 2, "heat").unwrap()["score"], 60);
}
