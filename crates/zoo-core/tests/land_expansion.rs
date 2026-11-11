//! #99: purchasable land. Construction outside owned land is rejected by the same plan its
//! preview shows; a purchase is one all-or-nothing transaction that never double-charges or
//! half-applies; expansion leaves the existing park, its IDs, routes, saves and framing alone;
//! and the authored parcels are different choices, not identical extra squares.
use serde_json::{Value, json};
use zoo_core::ZooGame;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) -> Value {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn refused(result: String, expected: &str) -> Value {
    let value = json(&result);
    assert_eq!(value["ok"], false, "{value}");
    let message = value["message"].as_str().unwrap();
    assert!(
        message.contains(expected),
        "{expected:?} not in {message:?}"
    );
    value
}

fn lakeside() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.start_scenario("partly-prepared"));
    game
}

/// Connects, zones, staffs and stocks the lakeside's existing habitat.
fn stock(game: &mut ZooGame) {
    ok(game.build_path_stroke(vec![12, 8, 13, 8], 1, "public".to_owned()));
    ok(game.create_work_zone(3, 6, 17, 9));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    ok(game.adopt(1, "capybara".to_owned()));
}

fn ids(parcels: &[&str]) -> Vec<String> {
    parcels.iter().map(|id| (*id).to_owned()).collect()
}

fn quote(game: &ZooGame, parcels: &[&str]) -> Value {
    json(&game.evaluate_land_purchase(ids(parcels)))
}

fn buy(game: &mut ZooGame, parcels: &[&str]) -> String {
    let token = quote(game, parcels)["quote"].as_str().unwrap().to_owned();
    game.buy_land(ids(parcels), token)
}

fn snapshot(game: &ZooGame) -> Value {
    json(&game.snapshot_json())
}

fn cash(game: &ZooGame) -> i64 {
    snapshot(game)["cash_cents"].as_i64().unwrap()
}

fn parcel(game: &ZooGame, id: &str) -> Value {
    snapshot(game)["land"]["parcels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parcel| parcel["id"] == id)
        .unwrap()
        .clone()
}

/// The ownership flags of every parcel, in authored order.
fn ownership(game: &ZooGame) -> Vec<bool> {
    snapshot(game)["land"]["parcels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|parcel| parcel["owned"].as_bool().unwrap())
        .collect()
}

#[test]
fn building_outside_owned_land_is_refused_until_the_parcel_is_bought() {
    let mut game = lakeside();
    // Finish the main path so habitats on the paddock have a path to face.
    ok(game.build_path_stroke(vec![12, 8, 17, 8], 1, "public".to_owned()));
    let before = cash(&game);

    // The preview names the parcel, its price and the tiles outside owned land.
    let stroke = json(&game.evaluate_path_stroke(vec![17, 8, 21, 8], 1, "public".to_owned()));
    assert_eq!(stroke["ok"], false);
    assert_eq!(
        stroke["message"],
        "Outside owned land: buy East Paddock ($9500) first"
    );
    assert_eq!(stroke["land_parcels"], json!(["east-paddock"]));
    assert_eq!(stroke["blocked_tiles"].as_array().unwrap().len(), 4);
    // The commit runs the same plan and spends nothing.
    refused(
        game.build_path_stroke(vec![17, 8, 21, 8], 1, "public".to_owned()),
        "Outside owned land",
    );
    refused(
        game.build_plaza(18, 9, 19, 10, "public".to_owned()),
        "East Paddock",
    );
    refused(
        game.place_concession(18, 9, "food".to_owned()),
        "East Paddock",
    );
    let habitat = json(&game.evaluate_habitat_rect(18, 9, 21, 12));
    assert_eq!(habitat["ok"], false);
    assert_eq!(habitat["land_parcels"], json!(["east-paddock"]));
    refused(game.place_habitat_rect(18, 9, 21, 12), "East Paddock");
    // Growing the existing habitat onto the lakeshore needs that parcel.
    let reshape = json(&game.evaluate_habitat_reshape(1, vec![13, 3, 19, 3, 19, 8, 13, 8, 13, 3]));
    assert_eq!(reshape["ok"], false);
    assert_eq!(reshape["land_parcels"], json!(["lakeshore"]));
    // A footprint across two unbought parcels names both.
    let both = json(&game.evaluate_path_stroke(vec![18, 6, 18, 9], 1, "public".to_owned()));
    assert_eq!(
        both["message"],
        "Outside owned land: buy Lakeshore and East Paddock ($14500) first"
    );
    assert_eq!(cash(&game), before, "refused construction spends nothing");

    let bought = ok(buy(&mut game, &["east-paddock"]));
    assert_eq!(bought["message"], "Bought East Paddock · $9500");
    assert_eq!(cash(&game), before - 950_000);
    let view = snapshot(&game);
    assert_eq!(
        view["finance"]["current_day"]["breakdown"]["land_expense_cents"],
        950_000
    );
    ok(game.build_path_stroke(vec![17, 8, 21, 8], 1, "public".to_owned()));
    ok(game.place_habitat_rect(18, 9, 21, 12));
    ok(game.place_concession(22, 8, "drink".to_owned()));
    // The lakeshore is still for sale, so the old fence still cannot grow onto it.
    refused(
        game.reshape_habitat(1, vec![13, 3, 19, 3, 19, 8, 13, 8, 13, 3]),
        "Lakeshore",
    );
}

#[test]
fn a_service_gate_never_opens_onto_unbought_land() {
    let mut game = lakeside();
    // Habitat 1's east fence faces the lakeshore at x = 18.
    let gate = json(&game.evaluate_service_gate(1, 17, 5));
    assert_eq!(gate["ok"], false, "{gate}");
    refused(game.set_service_gate(1, 17, 5, "east".to_owned()), "");
    ok(buy(&mut game, &["lakeshore"]));
    let gate = json(&game.evaluate_service_gate(1, 17, 5));
    assert_eq!(gate["ok"], true, "{gate}");
}

#[test]
fn purchases_are_atomic_and_never_double_charge() {
    let mut game = lakeside();
    let start = cash(&game);

    // An empty, unknown or repeated selection buys nothing.
    refused(buy(&mut game, &[]), "Select at least one parcel");
    refused(buy(&mut game, &["moon"]), "No parcel moon");
    let repeated = quote(&game, &["lakeshore", "lakeshore"]);
    assert_eq!(repeated["ok"], false);
    assert_eq!(repeated["lines"][1]["status"], "duplicate");
    refused(
        buy(&mut game, &["lakeshore", "lakeshore"]),
        "Selected more than once",
    );

    // A mixed selection (one buyable, one locked) is refused whole.
    let mixed = quote(&game, &["lakeshore", "south-meadow"]);
    assert_eq!(mixed["ok"], false);
    assert_eq!(mixed["lines"][0]["status"], "available");
    assert_eq!(mixed["lines"][1]["status"], "locked");
    assert_eq!(
        mixed["message"],
        "South Meadow: Locked: opens once a habitat has animals · nothing is bought"
    );
    refused(
        buy(&mut game, &["lakeshore", "south-meadow"]),
        "nothing is bought",
    );
    assert_eq!(ownership(&game), vec![false, false, false]);
    assert_eq!(cash(&game), start);

    // A garbage quote is never permission to spend.
    refused(
        game.buy_land(ids(&["lakeshore"]), "fnv1a64:0000000000000000".to_owned()),
        "quote is out of date",
    );

    // Two parcels together: one charge for the exact total.
    let pair = quote(&game, &["east-paddock", "lakeshore"]);
    assert_eq!(pair["ok"], true);
    assert_eq!(pair["total_cents"], 1_450_000);
    assert_eq!(pair["cash_after_cents"], start - 1_450_000);
    let token = pair["quote"].as_str().unwrap().to_owned();
    ok(game.buy_land(ids(&["east-paddock", "lakeshore"]), token.clone()));
    assert_eq!(cash(&game), start - 1_450_000);
    assert_eq!(ownership(&game), vec![true, true, false]);

    // Repeating the purchase, or presenting the same quote again, charges nothing.
    refused(
        game.buy_land(ids(&["east-paddock", "lakeshore"]), token),
        "Already owned",
    );
    refused(buy(&mut game, &["lakeshore"]), "Already owned");
    assert_eq!(cash(&game), start - 1_450_000);
    let ledger = &snapshot(&game)["finance"]["current_day"]["breakdown"];
    assert_eq!(ledger["land_expense_cents"], 1_450_000);
}

#[test]
fn a_stale_quote_is_refused_even_when_the_selection_became_buyable() {
    let mut game = lakeside();
    let locked = quote(&game, &["south-meadow"]);
    assert_eq!(locked["ok"], false);
    let stale = locked["quote"].as_str().unwrap().to_owned();

    // Stocking the existing habitat unlocks the meadow, which changes its quote.
    stock(&mut game);
    let open = quote(&game, &["south-meadow"]);
    assert_eq!(open["ok"], true, "{open}");
    assert_ne!(open["quote"], locked["quote"]);
    let before = cash(&game);
    refused(
        game.buy_land(ids(&["south-meadow"]), stale),
        "quote is out of date",
    );
    assert_eq!(cash(&game), before);
    assert_eq!(parcel(&game, "south-meadow")["owned"], false);
    ok(buy(&mut game, &["south-meadow"]));
    assert_eq!(parcel(&game, "south-meadow")["status"], "owned");
}

#[test]
fn expansion_keeps_the_existing_park_its_ids_routes_saves_and_framing() {
    let mut game = lakeside();
    ok(game.build_path_stroke(vec![12, 8, 13, 8], 1, "public".to_owned()));
    stock(&mut game);
    for _ in 0..4 {
        game.tick(30);
    }
    let before = snapshot(&game);
    let bundle = json(&game.diagnostics_json());
    ok(buy(&mut game, &["east-paddock", "south-meadow"]));
    let after = snapshot(&game);
    for key in [
        "width",
        "height",
        "entrance",
        "tiles",
        "habitats",
        "concessions",
        "animals",
        "guests",
        "work_zones",
        "scenario",
    ] {
        assert_eq!(after[key], before[key], "{key} changed by buying land");
    }
    let diagnostics = json(&game.diagnostics_json());
    assert_eq!(diagnostics["start"], bundle["start"]);

    // New construction continues the same ID sequences.
    ok(game.build_path_stroke(vec![17, 8, 21, 8], 1, "public".to_owned()));
    let built = ok(game.place_habitat_rect(18, 9, 21, 12));
    assert!(
        built["message"]
            .as_str()
            .unwrap()
            .starts_with("Habitat #2 fenced")
    );

    // Saves round-trip the ownership; a reloaded park still refuses unbought land.
    let saved = game.save_json();
    let mut loaded = ZooGame::new();
    ok(loaded.load_save(&saved));
    assert_eq!(loaded.save_json(), saved);
    assert_eq!(ownership(&loaded), vec![true, false, true]);
    refused(
        loaded.build_path_stroke(vec![18, 7], 1, "public".to_owned()),
        "Lakeshore",
    );

    // A save that claims construction on unbought land is rejected and changes nothing.
    let mut forged = json(&saved);
    forged["park"]["land"][0]["owned"] = false.into();
    let current = loaded.save_json();
    let result = json(&loaded.load_save(&forged.to_string()));
    assert_eq!(result["ok"], false);
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("built on unowned parcel east-paddock"),
        "{result}"
    );
    assert_eq!(loaded.save_json(), current);

    // The session replays the purchase exactly.
    let replayed = ZooGame::replay(&game.content_json(), &game.diagnostics_json()).unwrap();
    assert_eq!(replayed.snapshot_json(), game.snapshot_json());
}

#[test]
fn the_authored_parcels_are_different_choices() {
    let mut game = lakeside();
    let paddock = parcel(&game, "east-paddock");
    let lakeshore = parcel(&game, "lakeshore");
    let meadow = parcel(&game, "south-meadow");
    // The paddock is dry, open and reachable by extending the main path at once.
    assert_eq!(paddock["status"], "available");
    assert_eq!(paddock["path_frontage"], true);
    assert_eq!(paddock["water_tiles"], 0);
    // The lakeshore is cheaper but half lake, with no path reaching it yet.
    assert_eq!(lakeshore["status"], "available");
    assert!(lakeshore["price_cents"].as_i64() < paddock["price_cents"].as_i64());
    assert!(lakeshore["water_tiles"].as_u64().unwrap() > 0);
    assert!(lakeshore["buildable_tiles"].as_u64() < paddock["buildable_tiles"].as_u64());
    assert_eq!(lakeshore["path_frontage"], false);
    // The meadow is the largest and cheapest per tile, but locked until animals are on show.
    assert_eq!(meadow["status"], "locked");
    assert_eq!(
        meadow["restriction"],
        "Locked: opens once a habitat has animals"
    );
    assert!(meadow["buildable_tiles"].as_u64() > paddock["buildable_tiles"].as_u64());
    assert!(
        meadow["price_per_buildable_tile_cents"].as_i64()
            < paddock["price_per_buildable_tile_cents"].as_i64()
    );
    stock(&mut game);
    assert_eq!(parcel(&game, "south-meadow")["status"], "available");
    let land = &snapshot(&game)["land"];
    assert_eq!(land["total_tiles"], 24 * 16);
    assert_eq!(land["owned_tiles"], 24 * 16 - 48 - 48 - 72);
}
