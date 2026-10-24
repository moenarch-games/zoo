//! Public-API regressions for the September 2026 routing audit (#66–#69).
use serde_json::Value;
use zoo_core::ZooGame;

fn accepted(result: String) {
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["ok"], true, "{result}");
}
fn snapshot(game: &ZooGame) -> Value {
    serde_json::from_str(&game.snapshot_json()).unwrap()
}
/// Covers the park with one work zone (once) and assigns the newest keeper to it.
fn zone_newest_keeper(game: &mut ZooGame) {
    let view = snapshot(game);
    if view["work_zones"].as_array().unwrap().is_empty() {
        accepted(game.create_work_zone(0, 0, 19, 13));
    }
    let keeper = view["animal_care_depot"]["keepers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    accepted(game.assign_keeper_zone(keeper, 1));
}
fn populate(game: &mut ZooGame, rect: (u32, u32, u32, u32)) -> u32 {
    accepted(game.place_habitat_rect(rect.0, rect.1, rect.2, rect.3));
    let view = snapshot(game);
    let id = view["habitats"].as_array().unwrap().last().unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    accepted(game.hire_keeper());
    zone_newest_keeper(game);
    accepted(game.adopt(id, "capybara".to_owned()));
    id
}
fn populated_park() -> ZooGame {
    let mut game = ZooGame::new();
    populate(&mut game, (5, 5, 8, 7));
    game
}
fn guest(view: &Value, id: u32) -> Option<&Value> {
    view["guests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|guest| guest["id"] == id)
}
fn assert_position(actor: &Value, x: u32, y: u32) {
    assert_eq!(actor["x"], x, "{actor}");
    assert_eq!(actor["y"], y, "{actor}");
}
fn assert_walkable_guests(view: &Value) {
    for guest in view["guests"].as_array().unwrap() {
        let tile = view["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tile| tile["x"] == guest["x"] && tile["y"] == guest["y"])
            .unwrap();
        assert!(
            matches!(tile["kind"].as_str(), Some("path" | "entrance")),
            "guest entered a non-walkable tile: {guest}; {tile}"
        );
    }
}
fn advance(game: &mut ZooGame, minutes: u32) {
    for _ in 0..minutes {
        game.tick(1);
        assert_walkable_guests(&snapshot(game));
    }
}
fn await_guest_state(game: &mut ZooGame, id: u32, state: &str) {
    for _ in 0..240 {
        if guest(&snapshot(game), id).is_some_and(|guest| guest["state"] == state) {
            return;
        }
        advance(game, 1);
    }
    panic!("guest {id} did not reach {state}");
}
fn await_guest_thought(game: &mut ZooGame, id: u32, expected: &str) {
    for _ in 0..60 {
        if guest(&snapshot(game), id).is_some_and(|guest| {
            guest["thought"]
                .as_str()
                .is_some_and(|thought| thought.contains(expected))
        }) {
            return;
        }
        advance(game, 1);
    }
    panic!("guest {id} did not report {expected:?}");
}

#[test]
fn disconnected_habitat_does_not_starve_admissions_and_rejoins_after_connection() {
    let mut game = populated_park();
    accepted(game.place_path(10, 7));
    populate(&mut game, (11, 5, 14, 7));
    advance(&mut game, 240);
    let view = snapshot(&game);
    assert_eq!(view["entrance"]["arrivals_total"], 10);
    assert_eq!(
        view["finance"]["current_day"]["breakdown"]["admissions_income_cents"],
        12_000
    );
    assert!(
        view["guests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|guest| guest["target_habitat"] == 1)
    );
    for x in 4..=10 {
        accepted(game.place_path(x, 8));
    }
    advance(&mut game, 24);
    let view = snapshot(&game);
    assert_eq!(view["entrance"]["arrivals_total"], 11);
    assert_eq!(guest(&view, 11).unwrap()["target_habitat"], 2);
}
#[test]
fn all_unreachable_habitats_charge_nothing_and_preserve_the_first_guest_id() {
    let mut game = populated_park();
    accepted(game.bulldoze(3, 7));
    advance(&mut game, 48);
    let view = snapshot(&game);
    assert_eq!(view["entrance"]["arrivals_total"], 0);
    assert_eq!(
        view["finance"]["current_day"]["breakdown"]["admissions_income_cents"],
        0
    );
    accepted(game.place_path(3, 7));
    advance(&mut game, 24);
    let view = snapshot(&game);
    assert_eq!(view["entrance"]["arrivals_total"], 1);
    assert!(guest(&view, 1).is_some());
}
#[test]
fn isolated_first_border_path_does_not_hide_a_reachable_viewing_tile() {
    let mut game = populated_park();
    accepted(game.place_path(4, 5));
    advance(&mut game, 48);
    let view = snapshot(&game);
    assert_eq!(view["entrance"]["arrivals_total"], 2);
    let first = guest(&view, 1).unwrap();
    assert_position(first, 4, 7);
    assert_eq!(first["state"], "viewing");
}
#[test]
fn equally_short_viewing_routes_follow_deterministic_bfs_neighbor_order() {
    let mut first = ZooGame::new();
    let mut second = ZooGame::new();
    for game in [&mut first, &mut second] {
        accepted(game.place_path(2, 8));
        populate(game, (3, 8, 6, 10));
        advance(game, 45);
        let view = snapshot(game);
        // (3,7) and (2,8) are equally close. The existing BFS visits (3,7) first.
        assert_position(guest(&view, 1).unwrap(), 3, 7);
    }
    assert_eq!(first.snapshot_json(), second.snapshot_json());
}
#[test]
fn arriving_guest_waits_instead_of_crossing_a_replacement_concession() {
    let mut game = populated_park();
    advance(&mut game, 24);
    accepted(game.bulldoze(2, 7));
    accepted(game.place_concession(2, 7, "food".to_owned()));
    assert!(
        guest(&snapshot(&game), 1).unwrap()["thought"]
            .as_str()
            .unwrap()
            .contains("blocked")
    );
    advance(&mut game, 24);
    let view = snapshot(&game);
    let first = guest(&view, 1).unwrap();
    assert_position(first, 0, 7);
    assert_eq!(first["state"], "walking_to_habitat");
    accepted(game.bulldoze(2, 7));
    accepted(game.place_path(2, 7));
    advance(&mut game, 18);
    assert_eq!(guest(&snapshot(&game), 1).unwrap()["state"], "viewing");
}
#[test]
fn outbound_guest_reroutes_around_a_demolished_path_without_teleporting() {
    let mut game = populated_park();
    for x in 1..=3 {
        accepted(game.place_path(x, 8));
    }
    advance(&mut game, 33);
    assert_position(guest(&snapshot(&game), 1).unwrap(), 1, 7);
    accepted(game.bulldoze(2, 7));
    assert_position(guest(&snapshot(&game), 1).unwrap(), 1, 7);
    let mut used_bypass = false;
    for _ in 0..24 {
        advance(&mut game, 1);
        let view = snapshot(&game);
        let first = guest(&view, 1).unwrap();
        used_bypass |= first["x"] == 2 && first["y"] == 8;
    }
    assert!(used_bypass);
    assert_eq!(guest(&snapshot(&game), 1).unwrap()["state"], "viewing");
}
#[test]
fn outbound_guest_waits_in_place_and_resumes_after_a_path_edit() {
    let mut game = populated_park();
    advance(&mut game, 36);
    assert_position(guest(&snapshot(&game), 1).unwrap(), 2, 7);
    accepted(game.bulldoze(3, 7));
    advance(&mut game, 18);
    let view = snapshot(&game);
    let first = guest(&view, 1).unwrap();
    assert_position(first, 2, 7);
    assert_eq!(first["state"], "walking_to_habitat");
    assert!(first["thought"].as_str().unwrap().contains("blocked"));
    accepted(game.place_path(3, 7));
    advance(&mut game, 12);
    assert_eq!(guest(&snapshot(&game), 1).unwrap()["state"], "viewing");
}
#[test]
fn exiting_guest_is_not_removed_or_moved_through_a_broken_route() {
    let mut game = populated_park();
    await_guest_state(&mut game, 1, "walking_to_exit");
    let view = snapshot(&game);
    assert_position(guest(&view, 1).unwrap(), 4, 7);
    accepted(game.bulldoze(2, 7));
    accepted(game.place_concession(2, 7, "food".to_owned()));
    advance(&mut game, 18);
    let view = snapshot(&game);
    let first = guest(&view, 1).unwrap();
    assert_position(first, 4, 7);
    assert_eq!(first["state"], "walking_to_exit");
    assert!(
        first["thought"]
            .as_str()
            .unwrap()
            .contains("exit is blocked")
    );
    accepted(game.bulldoze(2, 7));
    accepted(game.place_path(2, 7));
    advance(&mut game, 18);
    assert!(guest(&snapshot(&game), 1).is_none());
}
#[test]
fn viewing_guest_reports_a_blocked_exit_and_recovers_after_reconnection() {
    let mut game = populated_park();
    advance(&mut game, 48);
    assert_eq!(guest(&snapshot(&game), 1).unwrap()["state"], "viewing");
    accepted(game.bulldoze(2, 7));
    await_guest_thought(&mut game, 1, "exit is blocked");
    let view = snapshot(&game);
    assert_position(guest(&view, 1).unwrap(), 4, 7);
    accepted(game.place_path(2, 7));
    advance(&mut game, 18);
    assert!(guest(&snapshot(&game), 1).is_none());
}
#[test]
fn demolishing_a_guest_occupied_path_is_rejected_atomically() {
    let mut game = populated_park();
    advance(&mut game, 36);
    assert_position(guest(&snapshot(&game), 1).unwrap(), 2, 7);
    let before = game.save_json();
    let result: Value = serde_json::from_str(&game.bulldoze(2, 7)).unwrap();
    assert_eq!(result["ok"], false);
    assert!(result["message"].as_str().unwrap().contains("guest"));
    assert_eq!(game.save_json(), before);
}
#[test]
fn both_staff_roles_spawn_on_the_connected_side_of_the_depot() {
    let mut game = ZooGame::new();
    accepted(game.place_path(2, 5));
    accepted(game.hire_janitor());
    accepted(game.hire_mechanic());
    let view = snapshot(&game);
    for role in ["janitors", "mechanics"] {
        assert_position(&view["animal_care_depot"][role][0], 2, 7);
    }
    // The isolated tile is not occupied by the newly hired staff.
    accepted(game.bulldoze(2, 5));
}
#[test]
fn staff_can_actually_service_the_main_path_despite_an_isolated_depot_neighbor() {
    let mut game = populated_park();
    accepted(game.place_path(2, 5));
    accepted(game.hire_janitor());
    accepted(game.hire_mechanic());
    accepted(game.place_concession(4, 6, "food".to_owned()));
    advance(&mut game, 240);
    let view = snapshot(&game);
    assert!(
        view["animal_care_depot"]["janitors"][0]["tasks_completed"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        view["animal_care_depot"]["mechanics"][0]["repairs_completed"]
            .as_u64()
            .unwrap()
            > 0
    );
}
#[test]
fn disconnected_depot_hiring_preserves_cash_ledger_and_staff_ids() {
    let mut game = ZooGame::new();
    accepted(game.bulldoze(2, 7));
    accepted(game.place_path(2, 5));
    let before = game.save_json();
    let janitor: Value = serde_json::from_str(&game.hire_janitor()).unwrap();
    assert_eq!(janitor["ok"], false);
    assert_eq!(game.save_json(), before);
    let mechanic: Value = serde_json::from_str(&game.hire_mechanic()).unwrap();
    assert_eq!(mechanic["ok"], false);
    assert_eq!(game.save_json(), before);
    accepted(game.place_path(2, 7));
    accepted(game.hire_janitor());
    accepted(game.hire_mechanic());
    let view = snapshot(&game);
    assert_eq!(view["animal_care_depot"]["janitors"][0]["id"], 1);
    assert_eq!(view["animal_care_depot"]["mechanics"][0]["id"], 1);
}
#[test]
fn routing_edits_and_staff_work_replay_to_identical_public_snapshots() {
    let mut first = populated_park();
    let mut second = populated_park();
    for game in [&mut first, &mut second] {
        accepted(game.place_path(10, 7));
        populate(game, (11, 5, 14, 7));
        accepted(game.place_path(4, 5));
        advance(game, 33);
        accepted(game.bulldoze(2, 7));
        advance(game, 18);
        accepted(game.place_path(2, 7));
        accepted(game.place_path(2, 5));
        accepted(game.hire_janitor());
        accepted(game.hire_mechanic());
        accepted(game.place_concession(4, 6, "food".to_owned()));
        advance(game, 180);
    }
    assert_eq!(first.snapshot_json(), second.snapshot_json());
}
