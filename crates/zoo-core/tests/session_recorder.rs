//! Independent #87 session-recorder acceptance (Atlas 4.1.1-4.1.5).
//!
//! Contract: `docs/acceptance/session-recorder.md`. Authored before implementation
//! against baseline 56f06ca; it exercises only the public `ZooGame` API that the spec
//! defines (`diagnostics_json`, `session_status`, `inject_fault`, `ZooGame::replay`) plus
//! the existing command, tick, reset and snapshot methods. No checkpoint (#88) is used.
use serde_json::{Value, json};
use std::collections::BTreeSet;
use zoo_core::ZooGame;

const MAX_COMMANDS: usize = 2_000;
const MAX_EVENTS: usize = 500;
const MAX_EXPORT_BYTES: usize = 1_048_576;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|error| panic!("invalid JSON ({error}): {text}"))
}
fn bundle(game: &ZooGame) -> Value {
    parse(&game.diagnostics_json())
}
fn snapshot(game: &ZooGame) -> Value {
    parse(&game.snapshot_json())
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
fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("expected object: {value}"))
        .keys()
        .cloned()
        .collect()
}
fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}
fn commands(bundle: &Value) -> &Vec<Value> {
    bundle["commands"].as_array().expect("commands array")
}
fn u(value: &Value) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("expected unsigned integer: {value}"))
}

/// The existing performance-journey park (`examples/performance_smoke.rs`): a staffed,
/// stocked habitat with concessions, janitor and mechanic, so guests and staff do work.
fn operated_park() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.place_habitat(3, 8, 0));
    ok(game.buy_animal_feed());
    ok(game.hire_keeper());
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.assign_keeper_zone(1, 1));
    ok(game.add_shelter(1));
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.adopt(1, "capybara".to_owned()));
    ok(game.place_concession(1, 6, "drink".to_owned()));
    ok(game.place_concession(3, 6, "food".to_owned()));
    ok(game.hire_janitor());
    ok(game.hire_mechanic());
    game
}

/// A scripted session mixing accepted and rejected commands with split and consecutive
/// advances, then an injected invariant violation that must be detected and frozen.
fn failing_session() -> ZooGame {
    let mut game = operated_park();
    game.tick(4);
    game.tick(4);
    game.tick(240);
    rejected(game.adopt(1, "unicorn".to_owned()));
    ok(game.place_path(5, 7));
    game.tick(1);
    rejected(game.place_habitat_rect(3, 5, 5, 8));
    game.tick(120);
    ok(game.inject_fault("invariant"));
    game.tick(30);
    game.tick(30);
    game
}

#[test]
fn bundle_declares_build_start_identity_clock_limits_and_only_declared_fields() {
    let mut game = operated_park();
    game.tick(60);
    let bundle = bundle(&game);
    assert_eq!(
        keys(&bundle),
        set(&[
            "format",
            "build",
            "start",
            "status",
            "clock",
            "limits",
            "commands",
            "commands_total",
            "commands_dropped",
            "events",
            "events_total",
            "events_dropped",
            "entities",
            "work",
            "failures",
            "truncated",
        ]),
        "the bundle must contain exactly the declared top-level fields"
    );
    assert_eq!(bundle["format"], "zoo-diagnostics/1");
    assert_eq!(keys(&bundle["build"]), set(&["zoo_core_version"]));
    assert!(
        bundle["build"]["zoo_core_version"]
            .as_str()
            .is_some_and(|version| !version.is_empty())
    );
    let content = parse(&game.content_json());
    assert_eq!(
        keys(&bundle["start"]),
        set(&["scenario", "ruleset_id", "schema_version", "content_digest"])
    );
    assert_eq!(bundle["start"]["scenario"], "default");
    assert_eq!(bundle["start"]["ruleset_id"], content["ruleset_id"]);
    assert_eq!(bundle["start"]["schema_version"], content["schema_version"]);
    assert_eq!(bundle["status"], "running");
    assert_eq!(game.session_status(), "running");
    let view = snapshot(&game);
    assert_eq!(
        keys(&bundle["clock"]),
        set(&["tick", "day", "minute_of_day"])
    );
    assert_eq!(u(&bundle["clock"]["tick"]), 60);
    assert_eq!(bundle["clock"]["day"], view["day"]);
    assert_eq!(bundle["clock"]["minute_of_day"], view["minute_of_day"]);
    assert_eq!(
        bundle["limits"],
        json!({"max_commands": 2000, "max_events": 500, "max_export_bytes": 1048576})
    );
    assert_eq!(bundle["failures"], json!([]));
    assert_eq!(bundle["truncated"], false);

    for command in commands(&bundle) {
        assert_eq!(
            keys(command),
            set(&["seq", "tick", "kind", "args", "result"])
        );
        assert_eq!(keys(&command["result"]), set(&["ok", "message"]));
        assert!(command["args"].is_object(), "{command}");
    }
    for event in bundle["events"].as_array().expect("events array") {
        assert_eq!(keys(event), set(&["seq", "tick", "kind", "entity_id"]));
    }
    assert_eq!(
        keys(&bundle["entities"]),
        set(&[
            "habitats",
            "concessions",
            "keepers",
            "janitors",
            "mechanics",
            "guests",
            "work_zones"
        ])
    );
    for counter in bundle["work"].as_object().expect("work object").values() {
        assert!(
            counter.is_u64(),
            "work counters are non-negative integers only: {counter}"
        );
    }
}

#[test]
fn start_identity_digest_follows_validated_content() {
    let first = bundle(&ZooGame::new());
    let again = bundle(&ZooGame::new());
    let digest = first["start"]["content_digest"]
        .as_str()
        .expect("digest string");
    assert!(!digest.is_empty());
    assert_eq!(
        again["start"]["content_digest"],
        first["start"]["content_digest"]
    );

    let mut definition = parse(&ZooGame::new().content_json());
    definition["species"][0]["label"] = json!("Edited acceptance label");
    let edited = ZooGame::from_content_json(&definition.to_string()).expect("valid content");
    assert_ne!(
        bundle(&edited)["start"]["content_digest"],
        first["start"]["content_digest"],
        "different content must not share a starting-state identity"
    );
}

#[test]
fn accepted_and_rejected_commands_are_recorded_in_order_with_results() {
    let mut game = ZooGame::new();
    ok(game.place_path(5, 7));
    rejected(game.adopt(1, "unicorn".to_owned()));
    ok(game.place_habitat(3, 8, 0));
    rejected(game.place_concession(0, 0, "no-such-stand".to_owned()));
    game.tick(5);
    let bundle = bundle(&game);
    let log = commands(&bundle);
    assert_eq!(u(&bundle["commands_total"]), log.len() as u64);
    assert_eq!(bundle["commands_dropped"], 0);
    let expected = [
        ("build_path_stroke", json!({"points": [5, 7]}), true),
        (
            "adopt",
            json!({"habitat_id": 1, "species": "unicorn"}),
            false,
        ),
        (
            "place_habitat",
            json!({"x": 3, "y": 8, "orientation": 0}),
            true,
        ),
        (
            "place_concession",
            json!({"x": 0, "y": 0, "kind": "no-such-stand"}),
            false,
        ),
        ("advance", json!({"minutes": 5}), true),
    ];
    assert_eq!(log.len(), expected.len(), "{log:?}");
    for (index, (entry, (kind, args, accepted))) in log.iter().zip(expected).enumerate() {
        assert_eq!(u(&entry["seq"]), index as u64, "{entry}");
        assert_eq!(entry["kind"], kind, "{entry}");
        assert_eq!(entry["args"], args, "{entry}");
        assert_eq!(entry["result"]["ok"], accepted, "{entry}");
        assert!(
            entry["result"]["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
            "{entry}"
        );
    }
    assert_eq!(log[1]["result"]["message"], "Unknown species");
    assert_eq!(
        u(&log[4]["tick"]),
        0,
        "advance is stamped with the tick it started at"
    );
}

#[test]
fn queries_and_previews_are_not_authoritative_commands() {
    let game = operated_park();
    let before = bundle(&game);
    let _ = game.snapshot_json();
    let _ = game.content_json();
    let _ = game.evaluate_habitat(10, 1, 0);
    let _ = game.evaluate_habitat_rect(6, 1, 11, 5);
    let _ = game.session_status();
    let _ = game.diagnostics_json();
    let after = bundle(&game);
    assert_eq!(
        after["commands"], before["commands"],
        "read-only queries, placement previews and diagnostics are not commands"
    );
    assert_eq!(after["commands_total"], before["commands_total"]);
    let kinds: BTreeSet<String> = commands(&after)
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap().to_owned())
        .collect();
    for forbidden in [
        "camera", "orbit", "zoom", "pan", "select", "hover", "audio", "snapshot",
    ] {
        assert!(
            kinds.iter().all(|kind| !kind.contains(forbidden)),
            "cosmetic/camera action recorded: {kinds:?}"
        );
    }
}

#[test]
fn replay_from_start_identity_and_ordered_log_reproduces_final_state_and_failing_step() {
    let original = failing_session();
    let recorded = original.diagnostics_json();
    let recorded_bundle = parse(&recorded);
    assert_eq!(recorded_bundle["status"], "frozen");
    assert_eq!(recorded_bundle["commands_dropped"], 0);
    let failure = recorded_bundle["failures"][0].clone();
    assert!(failure.is_object(), "{recorded_bundle}");

    // A second identical scripted run produces a byte-identical bundle.
    assert_eq!(failing_session().diagnostics_json(), recorded);

    let content = ZooGame::new().content_json();
    let mut previous: Option<(String, String)> = None;
    for _ in 0..3 {
        let replayed = ZooGame::replay(&content, &recorded).expect("complete log replays");
        let replayed_bundle = bundle(&replayed);
        assert_eq!(replayed.snapshot_json(), original.snapshot_json());
        assert_eq!(replayed_bundle["commands"], recorded_bundle["commands"]);
        assert_eq!(replayed_bundle["failures"], recorded_bundle["failures"]);
        assert_eq!(replayed_bundle["failures"][0]["seq"], failure["seq"]);
        assert_eq!(replayed_bundle["clock"], recorded_bundle["clock"]);
        assert_eq!(replayed.session_status(), "frozen");
        let run = (replayed.snapshot_json(), replayed.diagnostics_json());
        if let Some(previous) = &previous {
            assert_eq!(&run, previous, "repeated replays diverged");
        }
        previous = Some(run);
    }

    // Replaying a session without a failure also reproduces its final public state.
    let mut healthy = operated_park();
    healthy.tick(240);
    rejected(healthy.adopt(1, "unicorn".to_owned()));
    healthy.tick(240);
    let replayed = ZooGame::replay(&content, &healthy.diagnostics_json()).expect("replays");
    assert_eq!(replayed.snapshot_json(), healthy.snapshot_json());
    assert_eq!(replayed.session_status(), "running");
}

#[test]
fn replay_fails_closed_on_identity_mismatch_divergence_or_incomplete_log() {
    let recorded = failing_session().diagnostics_json();
    let content = ZooGame::new().content_json();

    let mut edited = parse(&content);
    edited["species"][0]["label"] = json!("Edited acceptance label");
    assert!(
        ZooGame::replay(&edited.to_string(), &recorded).is_err(),
        "content that does not match the recorded starting identity must not replay"
    );

    let mut tampered = parse(&recorded);
    let rejected_index = commands(&tampered)
        .iter()
        .position(|entry| entry["result"]["ok"] == false)
        .expect("a rejected command was recorded");
    tampered["commands"][rejected_index]["result"]["ok"] = json!(true);
    assert!(
        ZooGame::replay(&content, &tampered.to_string()).is_err(),
        "a re-executed result that differs from the recorded result is a divergence"
    );

    let mut game = ZooGame::new();
    for _ in 0..(MAX_COMMANDS + 1) {
        rejected(game.adopt(1, "unicorn".to_owned()));
    }
    assert!(
        ZooGame::replay(&content, &game.diagnostics_json()).is_err(),
        "a log whose oldest commands were dropped cannot be replayed from the initial state"
    );
}

#[test]
fn command_ring_keeps_exactly_the_latest_two_thousand() {
    let mut game = ZooGame::new();
    let total = MAX_COMMANDS + 500;
    for index in 0..total {
        if index % 2 == 0 {
            rejected(game.adopt(1, "unicorn".to_owned()));
        } else {
            ok(game.bulldoze(19, 13));
        }
    }
    let bundle = bundle(&game);
    let log = commands(&bundle);
    assert_eq!(log.len(), MAX_COMMANDS);
    assert_eq!(u(&bundle["commands_total"]), total as u64);
    assert_eq!(
        u(&bundle["commands_dropped"]),
        (total - MAX_COMMANDS) as u64
    );
    for (offset, entry) in log.iter().enumerate() {
        assert_eq!(u(&entry["seq"]), (total - MAX_COMMANDS + offset) as u64);
    }
    assert_eq!(log.last().unwrap()["kind"], "bulldoze");
    assert!(game.diagnostics_json().len() <= MAX_EXPORT_BYTES);
}

#[test]
fn event_ring_keeps_latest_five_hundred_with_stable_entity_ids() {
    let mut game = operated_park();
    for _ in 0..36 {
        game.tick(240); // six simulated days; about 60 guests arrive and leave per day
    }
    let bundle = bundle(&game);
    let events = bundle["events"].as_array().expect("events");
    let total = u(&bundle["events_total"]);
    assert!(
        total > MAX_EVENTS as u64,
        "long session produced only {total} events"
    );
    assert_eq!(events.len(), MAX_EVENTS);
    assert_eq!(u(&bundle["events_dropped"]), total - MAX_EVENTS as u64);
    for (offset, event) in events.iter().enumerate() {
        assert_eq!(u(&event["seq"]), total - MAX_EVENTS as u64 + offset as u64);
    }
    for pair in events.windows(2) {
        assert!(u(&pair[0]["tick"]) <= u(&pair[1]["tick"]));
    }

    let view = snapshot(&game);
    let guest_ids: Vec<u64> = view["guests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|guest| u(&guest["id"]))
        .collect();
    assert!(
        !guest_ids.is_empty(),
        "the operated park should have guests"
    );
    for id in &guest_ids {
        assert!(
            events
                .iter()
                .any(|event| event["kind"] == "guest_arrived" && event["entity_id"] == *id),
            "current guest #{id} has no guest_arrived event"
        );
    }
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "guest_left" && event["entity_id"].is_u64())
    );

    let mut sorted = guest_ids.clone();
    sorted.sort_unstable();
    assert_eq!(bundle["entities"]["guests"], json!(sorted));
    assert_eq!(bundle["entities"]["habitats"], json!([1]));
    assert_eq!(bundle["entities"]["concessions"], json!([1, 2]));
    assert_eq!(bundle["entities"]["keepers"], json!([1]));
    assert_eq!(bundle["entities"]["janitors"], json!([1]));
    assert_eq!(bundle["entities"]["mechanics"], json!([1]));
}

#[test]
fn work_counters_track_simulated_subsystem_work() {
    let mut game = operated_park();
    let idle = bundle(&game)["work"].clone();
    for name in ["simulated_minutes", "guest_updates", "path_searches"] {
        assert!(idle[name].is_u64(), "missing work counter {name}: {idle}");
    }
    assert_eq!(idle["simulated_minutes"], 0);
    game.tick(240);
    let after = bundle(&game);
    let work = &after["work"];
    assert_eq!(work["simulated_minutes"], after["clock"]["tick"]);
    assert_eq!(u(&work["simulated_minutes"]), 240);
    assert!(u(&work["guest_updates"]) > 0, "{work}");
    assert!(u(&work["path_searches"]) > 0, "{work}");
    game.tick(240);
    let later = bundle(&game)["work"].clone();
    for (name, value) in work.as_object().unwrap() {
        assert!(u(&later[name]) >= u(value), "work counter {name} decreased");
    }
}

#[test]
fn export_stays_within_one_megabyte_and_keeps_the_latest_command() {
    let mut game = operated_park();
    let small = game.diagnostics_json();
    assert!(small.len() <= MAX_EXPORT_BYTES);
    assert_eq!(parse(&small)["truncated"], false);

    let oversized = "z".repeat(4_096);
    let total = MAX_COMMANDS + 100;
    for _ in 0..total {
        rejected(game.adopt(1, oversized.clone()));
    }
    for _ in 0..12 {
        game.tick(240);
    }
    let text = game.diagnostics_json();
    assert!(
        text.len() <= MAX_EXPORT_BYTES,
        "diagnostics export is {} bytes, cap is {MAX_EXPORT_BYTES}",
        text.len()
    );
    let bundle = parse(&text);
    assert_eq!(
        bundle["truncated"], true,
        "shortening the export must be declared"
    );
    let total_commands = u(&bundle["commands_total"]);
    let last = commands(&bundle).last().expect("latest command retained");
    assert_eq!(u(&last["seq"]), total_commands - 1);
    assert_eq!(last["kind"], "advance");
    assert!(commands(&bundle).len() <= MAX_COMMANDS);
    assert!(bundle["events"].as_array().unwrap().len() <= MAX_EVENTS);
}

#[test]
fn detected_invariant_violation_freezes_without_looping_or_resetting() {
    let mut game = operated_park();
    game.tick(240);
    let healthy = snapshot(&game);
    ok(game.inject_fault("invariant"));
    game.tick(30);
    assert_eq!(game.session_status(), "frozen");
    let frozen_bundle = bundle(&game);
    assert_eq!(frozen_bundle["status"], "frozen");
    let failures = frozen_bundle["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1, "{failures:?}");
    let failure = &failures[0];
    assert_eq!(
        keys(failure),
        set(&["seq", "tick", "source", "kind", "message", "injected"])
    );
    assert_eq!(failure["source"], "simulation");
    assert_eq!(failure["kind"], "invariant_violation");
    assert_eq!(failure["injected"], true);
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
    let failing_seq = u(&failure["seq"]);
    assert!(
        commands(&frozen_bundle)
            .iter()
            .any(|entry| u(&entry["seq"]) == failing_seq),
        "the failure names the recorded command during which it was detected"
    );
    assert!(
        commands(&frozen_bundle)
            .iter()
            .any(|entry| entry["kind"] == "inject_fault"
                && entry["args"] == json!({"kind": "invariant"})),
        "an injected fault is part of the recorded authoritative input"
    );
    assert!(
        commands(&frozen_bundle)
            .iter()
            .any(|entry| entry["kind"] == "hire_mechanic"),
        "pre-failure commands are retained"
    );

    let frozen_view = game.snapshot_json();
    let frozen_clock = frozen_bundle["clock"].clone();
    for _ in 0..50 {
        game.tick(240);
    }
    let result = rejected(game.hire_keeper());
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("frozen"),
        "{result}"
    );
    rejected(game.place_path(6, 7));
    assert_eq!(
        game.snapshot_json(),
        frozen_view,
        "a frozen session must not advance or mutate"
    );
    let after = bundle(&game);
    assert_eq!(after["clock"], frozen_clock);
    assert_eq!(
        after["failures"].as_array().unwrap().len(),
        1,
        "failure handling looped"
    );
    assert_eq!(after["status"], "frozen");

    // No auto-reset: the park the player built is still there.
    let view = snapshot(&game);
    assert_eq!(view["habitats"].as_array().unwrap().len(), 1);
    assert_eq!(view["day"], healthy["day"]);
    assert_eq!(view["concessions"].as_array().unwrap().len(), 2);
    assert_eq!(
        view["animals"].as_array().unwrap().len(),
        healthy["animals"].as_array().unwrap().len()
    );
    assert!(u(&view["minute_of_day"]) >= u(&healthy["minute_of_day"]));
    assert_ne!(
        view,
        snapshot(&ZooGame::new()),
        "frozen park must not be reset"
    );
}

#[test]
fn fault_injection_rejects_unknown_kinds_and_explicit_reset_starts_a_new_session() {
    let mut game = operated_park();
    rejected(game.inject_fault("not-a-fault"));
    game.tick(10);
    assert_eq!(game.session_status(), "running");

    ok(game.inject_fault("invariant"));
    game.tick(10);
    assert_eq!(game.session_status(), "frozen");

    // Only an explicit player reset leaves the frozen session; it begins a fresh recording.
    game.reset();
    assert_eq!(game.session_status(), "running");
    let fresh = bundle(&game);
    assert_eq!(fresh["status"], "running");
    assert_eq!(fresh["failures"], json!([]));
    assert_eq!(fresh["commands"], json!([]));
    assert_eq!(fresh["commands_total"], 0);
    assert_eq!(fresh["clock"]["tick"], 0);
    assert_eq!(
        fresh["start"],
        bundle(&ZooGame::new())["start"],
        "a reset session starts from the same identified initial state"
    );
    ok(game.place_path(5, 7));
    assert_eq!(game.snapshot_json(), {
        let mut expected = ZooGame::new();
        ok(expected.place_path(5, 7));
        expected.snapshot_json()
    });
}

#[test]
fn bundle_contains_no_credentials_or_external_data() {
    let mut game = operated_park();
    game.tick(120);
    rejected(game.adopt(1, "unicorn".to_owned()));
    let text = game.diagnostics_json();
    for forbidden in [
        "cookie",
        "password",
        "token",
        "secret",
        "authorization",
        "localStorage",
        "user_agent",
        "userAgent",
        "http://",
        "https://",
        "file://",
        "/home/",
    ] {
        assert!(
            !text.to_lowercase().contains(&forbidden.to_lowercase()),
            "diagnostics bundle contains {forbidden:?}"
        );
    }
}
