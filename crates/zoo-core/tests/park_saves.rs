//! #88: versioned saves restore the complete authoritative park, and a reloaded park continues
//! exactly like one that never stopped.
use serde_json::Value;
use zoo_core::ZooGame;

const FIXTURE_V1: &str = "tests/fixtures/save-v1.json";
const FIXTURE_V2: &str = "tests/fixtures/save-v2.json";
const FIXTURE_V3: &str = "tests/fixtures/save-v3.json";
const FIXTURE_V4: &str = "tests/fixtures/save-v4.json";
const FIXTURE_V5: &str = "tests/fixtures/save-v5.json";
const FIXTURE_V6: &str = "tests/fixtures/save-v6.json";
const FIXTURE_V7: &str = "tests/fixtures/save-v7.json";
const FIXTURE_V8: &str = "tests/fixtures/save-v8.json";
const FIXTURE_V9: &str = "tests/fixtures/save-v9.json";
const FIXTURE_V10: &str = "tests/fixtures/save-v10.json";
const FIXTURE_V11: &str = "tests/fixtures/save-v11.json";
const FIXTURE_V12: &str = "tests/fixtures/save-v12.json";
const FIXTURE_V13: &str = "tests/fixtures/save-v13.json";
const FIXTURE_V14: &str = "tests/fixtures/save-v14.json";
const FIXTURE_V15: &str = "tests/fixtures/save-v15.json";
const FIXTURE_V16: &str = "tests/fixtures/save-v16.json";
const FIXTURE_V17: &str = "tests/fixtures/save-v17.json";
const FIXTURE_V18: &str = "tests/fixtures/save-v18.json";
const FIXTURE_V19: &str = "tests/fixtures/save-v19.json";
const FIXTURE_V20: &str = "tests/fixtures/save-v20.json";
const FIXTURE_V21: &str = "tests/fixtures/save-v21.json";
const FIXTURE_V22: &str = "tests/fixtures/save-v22.json";

/// Undoes the schema 9 catalog change (#97) on a catalog document: the basic shelter's price
/// becomes the flat shelter price again and environment content disappears.
fn content_without_environment(content: &mut Value) {
    let shelter = content["habitat_objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["key"] == "basic_shelter")
        .unwrap()["build_cents"]
        .clone();
    let object = content.as_object_mut().unwrap();
    object.remove("habitat_objects").unwrap();
    let balance = object["balance"].as_object_mut().unwrap();
    balance.remove("habitat_environment").unwrap();
    balance["habitat_care"]
        .as_object_mut()
        .unwrap()
        .insert("shelter_cents".to_owned(), shelter);
    for species in object["species"].as_array_mut().unwrap() {
        species.as_object_mut().unwrap().remove("habitat").unwrap();
    }
}

/// Undoes the schema 11 → 12 migration (#101) on a document saved straight after loading: the
/// catalog loses facility capacities and the added toilets, bench and bin, stands lose their
/// comfort state, guests their toilet need and errands, janitors their tagged targets, and the
/// park its (empty) facility service queue.
const COMFORT_FACILITIES: [&str; 3] = ["toilet", "bench", "bin"];

/// Removes the built-in mixed-exhibit profiles (#108) from a catalog document.
fn content_without_compatibility(content: &mut Value) {
    content
        .as_object_mut()
        .unwrap()
        .remove("species_compatibility")
        .unwrap();
}

/// The catalog without the vet and medicine prices (#109).
fn content_without_health(content: &mut Value) {
    let balance = content["balance"].as_object_mut().unwrap();
    balance.remove("medicine_batch_cents").unwrap();
    balance["staff"]
        .as_object_mut()
        .unwrap()
        .remove("vet")
        .unwrap();
}

/// The catalog without utility plants or species utility demand (#113).
fn content_without_utilities(content: &mut Value) {
    content["facilities"]
        .as_array_mut()
        .unwrap()
        .retain(|facility| facility.get("utility").is_none());
    for species in content["species"].as_array_mut().unwrap() {
        species.as_object_mut().unwrap().remove("utilities");
    }
}

/// The catalog without the education sign or sourced facts (#116).
fn content_without_education(content: &mut Value) {
    content["facilities"]
        .as_array_mut()
        .unwrap()
        .retain(|facility| facility["service"] != "sign");
    content.as_object_mut().unwrap().remove("facts");
}

/// Undoes the schema 21 → 22 migration (#118) on a document saved straight after loading: the
/// park loses its fair-weather calendar, with both rules on, nothing announced and every day up
/// to the one after the saved one considered.
fn conditions_removed(value: &mut Value) {
    let park = value["park"].as_object_mut().unwrap();
    let day = park["day"].as_u64().unwrap();
    let conditions = park.remove("conditions").unwrap();
    assert_eq!(
        conditions,
        serde_json::json!({
            "definition": null,
            "rules": { "weather_effects": true, "events": true },
            "scheduled": null,
            "planned_through_day": day + 1,
            "next_event_day": day,
            "history": [],
        })
    );
    value["schema_version"] = 21.into();
}

/// Undoes the schema 20 → 21 migration (#116) on a document saved straight after loading: the
/// catalog loses the appended education sign and the sourced facts, no guest has learned
/// anything, the ledgers lose their zero donation and grant lines, and the park its untouched
/// education record.
fn education_removed(value: &mut Value) {
    conditions_removed(value);
    content_without_education(&mut value["content"]);
    let park = value["park"].as_object_mut().unwrap();
    let education = park.remove("education").unwrap();
    assert_eq!(education["talks"], serde_json::json!([]));
    assert_eq!(education["achievements"], serde_json::json!([]));
    assert_eq!(education["total"]["guests_educated"], 0);
    for guest in park["guests"].as_array_mut().unwrap() {
        assert_eq!(
            guest.as_object_mut().unwrap().remove("lessons").unwrap(),
            serde_json::json!([])
        );
    }
    let strip = |ledger: &mut Value| {
        let ledger = ledger.as_object_mut().unwrap();
        for field in ["donation_income_cents", "grant_income_cents"] {
            assert_eq!(ledger.remove(field).unwrap(), 0);
        }
    };
    strip(&mut park["finance_today"]);
    for day in park["finance"]["history"].as_array_mut().unwrap() {
        strip(&mut day["ledger"]);
    }
    value["schema_version"] = 20.into();
}

/// Undoes the schema 19 → 20 migration (#115) on a document saved straight after loading: the
/// ledgers lose their zero loan lines, the history's one day (if any) becomes the previous
/// day's ledger again with exactly the cash the ledgers explain, and nothing is owed or was in
/// deficit.
fn finances_removed(value: &mut Value) {
    education_removed(value);
    let park = value["park"].as_object_mut().unwrap();
    let profit = |ledger: &Value| -> i64 {
        ledger
            .as_object()
            .unwrap()
            .iter()
            .map(|(field, cents)| {
                let cents = cents.as_i64().unwrap();
                if field.ends_with("_income_cents") {
                    cents
                } else {
                    -cents
                }
            })
            .sum()
    };
    let strip = |ledger: &mut Value| {
        let ledger = ledger.as_object_mut().unwrap();
        for field in [
            "loan_interest_expense_cents",
            "loan_proceeds_cents",
            "loan_repayment_cents",
        ] {
            assert_eq!(ledger.remove(field).unwrap(), 0);
        }
    };
    let mut finance = park.remove("finance").unwrap();
    strip(&mut park["finance_today"]);
    let opening = park["cash_cents"].as_i64().unwrap() - profit(&park["finance_today"]);
    assert_eq!(finance["opening_cash_cents"], opening);
    let loan = &finance["loan"];
    for field in [
        "balance_cents",
        "peak_cents",
        "borrowed_total_cents",
        "repaid_total_cents",
        "interest_total_cents",
    ] {
        assert_eq!(loan[field], 0);
    }
    assert_eq!(finance["deficit_days"], 0);
    assert_eq!(finance["deficit_streak_days"], 0);
    let history = finance["history"].as_array_mut().unwrap();
    assert!(history.len() <= 1);
    let previous = match history.pop() {
        None => Value::Null,
        Some(mut day) => {
            assert_eq!(day["day"], park["day"].as_u64().unwrap() - 1);
            assert_eq!(day["closing_cash_cents"], opening);
            assert_eq!(day["loan_balance_cents"], 0);
            strip(&mut day["ledger"]);
            assert_eq!(day["opening_cash_cents"], opening - profit(&day["ledger"]));
            day["ledger"].take()
        }
    };
    park.insert("finance_previous".to_owned(), previous);
    value["schema_version"] = 19.into();
}

/// Undoes the schema 18 → 19 migration (#113) on a document saved straight after loading: the
/// catalog loses the appended water pump and climate plant, every habitat its utilities (normal
/// priority, nothing needed, since older species draw none), and the ledgers their utility
/// lines.
fn utilities_removed(value: &mut Value) {
    finances_removed(value);
    // The migration appends the plants. Parks from before schema 6 took the then-current
    // built-in catalog when migrated, which already lists them in place.
    let facilities = value["content"]["facilities"].as_array_mut().unwrap();
    let keys: Vec<&str> = facilities
        .iter()
        .map(|facility| facility["key"].as_str().unwrap())
        .collect();
    if keys.ends_with(&["water_pump", "climate_plant"]) {
        facilities.truncate(facilities.len() - 2);
    } else {
        assert!(keys.contains(&"water_pump") && keys.contains(&"climate_plant"));
    }
    let park = value["park"].as_object_mut().unwrap();
    let empty = serde_json::json!({
        "demand": 0, "supplied": 0, "shortage_minutes": 0, "rank": 0,
        "reason": null, "draws": [], "reachable": []
    });
    for habitat in park["habitats"].as_array_mut().unwrap() {
        assert_eq!(
            habitat
                .as_object_mut()
                .unwrap()
                .remove("utilities")
                .unwrap(),
            serde_json::json!({ "priority": "normal", "water": empty, "climate": empty })
        );
    }
    for key in ["finance_today", "finance_previous"] {
        if let Some(ledger) = park[key].as_object_mut() {
            for field in [
                "utility_operation_expense_cents",
                "utility_replacement_expense_cents",
            ] {
                assert_eq!(ledger.remove(field).unwrap(), 0);
            }
        }
    }
    value["schema_version"] = 18.into();
}

/// Undoes the schema 17 → 18 migration (#109) on a document saved straight after loading:
/// every animal was healthy with no risk, no habitat was a quarantine, every worker a keeper
/// with no kit in hand, the depot held no medicine, no job had a patient, no ledger had
/// veterinary costs, and the catalog no vet or medicine prices.
fn health_removed(value: &mut Value) {
    utilities_removed(value);
    content_without_health(&mut value["content"]);
    let park = value["park"].as_object_mut().unwrap();
    assert_eq!(park.remove("medicine_kits").unwrap(), 0);
    for habitat in park["habitats"].as_array_mut().unwrap() {
        let habitat = habitat.as_object_mut().unwrap();
        assert_eq!(habitat.remove("quarantine").unwrap(), false);
        for animal in habitat["animals"].as_array_mut().unwrap() {
            assert_eq!(
                animal.as_object_mut().unwrap().remove("health").unwrap(),
                serde_json::json!({
                    "risk": { "upset_tummy": 0, "sniffles": 0 },
                    "illness": null,
                    "quarantine_home": null
                })
            );
        }
    }
    for keeper in park["keepers"].as_array_mut().unwrap() {
        let keeper = keeper.as_object_mut().unwrap();
        assert_eq!(keeper.remove("role").unwrap(), "keeper");
        assert_eq!(keeper.remove("carrying_medicine").unwrap(), false);
    }
    for job in park["care_jobs"].as_array_mut().unwrap() {
        assert_eq!(
            job.as_object_mut().unwrap().remove("animal_id").unwrap(),
            Value::Null
        );
    }
    for key in ["finance_today", "finance_previous"] {
        if let Some(ledger) = park[key].as_object_mut() {
            for field in [
                "vet_hiring_expense_cents",
                "vet_wages_expense_cents",
                "medicine_expense_cents",
            ] {
                assert_eq!(ledger.remove(field).unwrap(), 0);
            }
        }
    }
    value["schema_version"] = 17.into();
}

/// Undoes the schema 16 → 17 migration (#108) on a document saved straight after loading:
/// every habitat had at most one species with its welfare (a habitat without animals kept no
/// species and the welfare a new habitat starts at), and the catalog no mixed-exhibit
/// profiles.
fn residents_removed(value: &mut Value) {
    health_removed(value);
    content_without_compatibility(&mut value["content"]);
    for habitat in value["park"]["habitats"].as_array_mut().unwrap() {
        let habitat = habitat.as_object_mut().unwrap();
        let residents = habitat.remove("residents").unwrap();
        let residents = residents.as_array().unwrap();
        assert!(residents.len() <= 1, "older parks keep one species");
        let (species, welfare) = residents.first().map_or((Value::Null, 100.into()), |r| {
            (r["species"].clone(), r["welfare"].clone())
        });
        habitat.insert("species".to_owned(), species);
        habitat.insert("welfare".to_owned(), welfare);
    }
    value["schema_version"] = 16.into();
}

/// Undoes the schema 15 → 16 migration (#106) on a document saved straight after loading: the
/// park charged its catalog's admission price, was open, had counted nobody at the gate and
/// run no campaign; stands charged their definition price; a guest at a window had agreed that
/// price and no other visit any; no ledger had price complaints or marketing.
fn pricing_removed(value: &mut Value) {
    residents_removed(value);
    let admission = value["content"]["balance"]["admission_price_cents"].clone();
    let park = value["park"].as_object_mut().unwrap();
    let pricing = park.remove("pricing").unwrap();
    assert_eq!(pricing["admission_cents"], admission);
    assert_eq!(pricing["open"], true);
    assert_eq!(pricing["today"]["groups_at_gate"], 0);
    assert_eq!(pricing["total"]["people_at_gate"], 0);
    assert_eq!(pricing["recent"], serde_json::json!([]));
    assert_eq!(pricing["campaign"], Value::Null);
    let mut prices = std::collections::HashMap::new();
    for stand in park["concessions"].as_array_mut().unwrap() {
        let price = stand["definition"]["price_cents"].clone();
        let id = stand["id"].clone();
        let Some(ops) = stand["stand"].as_object_mut() else {
            continue;
        };
        assert_eq!(ops.remove("price_cents").unwrap(), price);
        for period in ["today", "total"] {
            let lost = ops[period]["lost"].as_object_mut().unwrap();
            assert_eq!(lost.remove("too_expensive").unwrap(), 0);
        }
        prices.insert(id.to_string(), price);
    }
    for guest in park["guests"].as_array_mut().unwrap() {
        let serving = guest["state"] == "using_facility";
        let Some(visit) = guest["visit"].as_object_mut() else {
            continue;
        };
        let agreed = visit.remove("price_cents").unwrap();
        let expected = prices
            .get(&visit["facility_id"].to_string())
            .filter(|_| serving)
            .cloned()
            .unwrap_or_else(|| 0.into());
        assert_eq!(agreed, expected, "{visit:?}");
    }
    for key in ["finance_today", "finance_previous"] {
        if let Some(ledger) = park[key].as_object_mut() {
            assert_eq!(ledger.remove("marketing_expense_cents").unwrap(), 0);
        }
    }
    value["schema_version"] = 15.into();
}

/// Undoes the schema 14 → 15 migration (#104) on a document saved straight after loading: a
/// job's supplies were a flag (a carried crate or prepaid cleaning), and only the worker of an
/// in-progress feeding job that held its crate carried feed.
fn physical_care_removed(value: &mut Value) {
    pricing_removed(value);
    let park = value["park"].as_object_mut().unwrap();
    let mut carriers = Vec::new();
    for job in park["care_jobs"].as_array_mut().unwrap() {
        let job = job.as_object_mut().unwrap();
        let supply = job.remove("supply").unwrap();
        let reserved = match supply.as_str().unwrap() {
            "feed_carried" => {
                assert_eq!(job["category"], "feeding");
                if job["state"] == "in_progress" {
                    carriers.push(job["keeper_ids"][0].clone());
                }
                true
            }
            "cleaning_prepaid" => true,
            "none" => false,
            other => panic!("schema 14 held no {other} supply"),
        };
        job.insert("supplies_reserved".to_owned(), reserved.into());
    }
    for keeper in park["keepers"].as_array_mut().unwrap() {
        let keeper = keeper.as_object_mut().unwrap();
        let carrying = keeper.remove("carrying_feed").unwrap();
        assert_eq!(carrying, carriers.contains(&keeper["id"]), "{keeper:?}");
    }
    value["schema_version"] = 14.into();
}

/// Undoes the schema 13 → 14 migration (#103) on a document saved straight after loading:
/// every guest was its own group of one (IDs in guest order, admitted alone, no plan), started
/// uncrowded with no exit decided, and the visit record counted those arrivals and no exits.
fn groups_removed(value: &mut Value) {
    physical_care_removed(value);
    let park = value["park"].as_object_mut().unwrap();
    let groups = park.remove("groups").unwrap();
    let groups = groups.as_array().unwrap();
    let arrived = park["next_guest_id"].as_u64().unwrap() - 1;
    let guests = park["guests"].as_array_mut().unwrap();
    assert_eq!(groups.len(), guests.len());
    for (index, (guest, group)) in guests.iter_mut().zip(groups).enumerate() {
        let guest = guest.as_object_mut().unwrap();
        let group_id = index as u64 + 1;
        assert_eq!(guest.remove("group_id"), Some(group_id.into()));
        assert_eq!(guest.remove("crowd_minutes"), Some(0.into()));
        assert_eq!(guest.remove("exit"), Some(Value::Null));
        assert_eq!(group["id"], group_id);
        assert_eq!(group["members"], serde_json::json!([guest["id"]]));
        assert_eq!(group["admitted"], 1);
        assert_eq!(group["favorite_species"], Value::Null);
        assert_eq!(group["waiting_minutes"], 0);
        assert_eq!(group["plan"], "");
    }
    assert_eq!(
        park.remove("next_group_id"),
        Some((groups.len() as u64 + 1).into())
    );
    let visits = park.remove("visits").unwrap();
    assert_eq!(visits["groups_arrived"], arrived);
    assert_eq!(visits["total"], visits["today"]);
    assert_eq!(visits["recent"], serde_json::json!([]));
    value["schema_version"] = 13.into();
}

/// Undoes the schema 13 catalog change (#102) on a catalog document: stands lose their service
/// model and staff costs the vendor wage.
fn content_without_stands(content: &mut Value) {
    for facility in content["facilities"].as_array_mut().unwrap() {
        let facility = facility.as_object_mut().unwrap();
        let sells = matches!(facility["service"].as_str(), Some("food" | "drink"));
        assert_eq!(facility.remove("stand").is_some(), sells, "{facility:?}");
    }
    content["balance"]["staff"]
        .as_object_mut()
        .unwrap()
        .remove("vendor")
        .unwrap();
}

/// Undoes the schema 12 → 13 migration (#102) on a document saved straight after loading: the
/// catalog and every built stand's definition lose the service model, stands their operating
/// state (one vendor, full stock, no queue, a ledger carried over from their sales), guests
/// their spending money and ledgers the vendor-wage and stock lines.
fn staffed_stands_removed(value: &mut Value) {
    groups_removed(value);
    content_without_stands(&mut value["content"]);
    let park = value["park"].as_object_mut().unwrap();
    for stand in park["concessions"].as_array_mut().unwrap() {
        let stand = stand.as_object_mut().unwrap();
        let sells = stand["definition"].as_object_mut().unwrap().remove("stand");
        let state = stand.remove("stand").unwrap();
        match sells {
            None => assert_eq!(state, Value::Null),
            Some(service) => {
                assert_eq!(state["open"], true);
                assert_eq!(state["vendors"], 1);
                assert_eq!(state["stock"], service["stock_capacity"]);
                assert_eq!(state["queue"], serde_json::json!([]));
                assert_eq!(state["delivery_due"], Value::Null);
                assert_eq!(state["total"]["served"], stand["total_sales"]);
                assert_eq!(
                    state["total"]["revenue_cents"],
                    stand["total_revenue_cents"]
                );
                assert_eq!(state["today"]["served"], stand["sales_today"]);
            }
        }
    }
    for guest in park["guests"].as_array_mut().unwrap() {
        let guest = guest.as_object_mut().unwrap();
        let id = guest["id"].as_i64().unwrap();
        assert_eq!(
            guest.remove("cash_cents").unwrap(),
            serde_json::json!(1_000 + (id % 5) * 300)
        );
    }
    for key in ["finance_today", "finance_previous"] {
        if let Some(ledger) = park.get_mut(key).and_then(Value::as_object_mut) {
            for line in ["vendor_wages_expense_cents", "stand_stock_expense_cents"] {
                assert_eq!(ledger.remove(line), Some(0.into()));
            }
        }
    }
    value["schema_version"] = 12.into();
}

/// Undoes the schema 12 catalog change (#101) on a catalog document.
fn content_without_comfort(content: &mut Value) {
    if content["balance"]["staff"].get("vendor").is_some() {
        content_without_stands(content);
    }
    let facilities = content["facilities"].as_array_mut().unwrap();
    facilities.retain(|facility| !COMFORT_FACILITIES.contains(&facility["key"].as_str().unwrap()));
    for facility in facilities {
        assert_eq!(
            facility.as_object_mut().unwrap().remove("capacity"),
            Some(0.into())
        );
    }
}

fn comfort_removed(value: &mut Value) {
    const ADDED: [&str; 3] = COMFORT_FACILITIES;
    staffed_stands_removed(value);
    content_without_comfort(&mut value["content"]);
    let park = value["park"].as_object_mut().unwrap();
    if let Some(allowed) = park["unlocks"]
        .get_mut("facilities")
        .and_then(Value::as_array_mut)
    {
        allowed.retain(|key| !ADDED.contains(&key.as_str().unwrap()));
    }
    for stand in park["concessions"].as_array_mut().unwrap() {
        let stand = stand.as_object_mut().unwrap();
        stand["definition"]
            .as_object_mut()
            .unwrap()
            .remove("capacity")
            .unwrap();
        assert_eq!(stand.remove("slots"), Some(serde_json::json!([])));
        assert_eq!(stand.remove("cleanliness"), Some(100.into()));
        assert_eq!(stand.remove("fill"), Some(0.into()));
        assert_eq!(stand.remove("total_uses"), Some(0.into()));
    }
    for guest in park["guests"].as_array_mut().unwrap() {
        let guest = guest.as_object_mut().unwrap();
        assert_eq!(guest.remove("bladder"), Some(10.into()));
        assert_eq!(guest.remove("visit"), Some(Value::Null));
        assert_eq!(guest.remove("unmet_need"), Some(Value::Null));
        assert_eq!(guest.remove("next_need_check"), Some(0.into()));
    }
    for janitor in park["janitors"].as_array_mut().unwrap() {
        let janitor = janitor.as_object_mut().unwrap();
        let target = janitor.remove("target").unwrap();
        janitor.insert("target_litter_id".to_owned(), target["litter"].clone());
    }
    assert_eq!(park.remove("facility_service"), Some(serde_json::json!([])));
    assert_eq!(park.remove("next_facility_service_id"), Some(1.into()));
    value["schema_version"] = 11.into();
}

/// Undoes the schema 10 → 11 migration (#99) on a document whose park owns its whole map: the
/// park forgets its (empty) land and its ledgers lose the land-purchase line.
fn land_removed(value: &mut Value) {
    comfort_removed(value);
    let park = value["park"].as_object_mut().unwrap();
    assert_eq!(park.remove("land").unwrap(), serde_json::json!([]));
    for key in ["finance_today", "finance_previous"] {
        if let Some(ledger) = park.get_mut(key).and_then(Value::as_object_mut) {
            assert_eq!(
                ledger.remove("land_expense_cents").unwrap(),
                serde_json::json!(0)
            );
        }
    }
    value["schema_version"] = 10.into();
}

/// Undoes the schema 9 → 10 migration (#98) on a document without staff-only paths: every
/// public path tile is a plain `"path"` again.
fn paths_unclassified(value: &mut Value) {
    land_removed(value);
    for tile in value["park"]["tiles"].as_array_mut().unwrap() {
        assert_ne!(
            *tile,
            serde_json::json!({ "path": "staff" }),
            "staff path in an old park"
        );
        if *tile == serde_json::json!({ "path": "public" }) {
            *tile = "path".into();
        }
    }
    value["schema_version"] = 9.into();
}

/// Undoes the schema 8 → 9 migration (#97) on a saved document made without terrain edits:
/// a habitat with a shelter object has a shelter again and environments disappear.
fn environments_as_shelters(value: &mut Value) {
    paths_unclassified(value);
    for habitat in value["park"]["habitats"].as_array_mut().unwrap() {
        if habitat["species"].is_object() {
            habitat["species"]
                .as_object_mut()
                .unwrap()
                .remove("habitat")
                .unwrap();
        }
        let habitat = habitat.as_object_mut().unwrap();
        let environment = habitat.remove("environment").unwrap();
        assert_eq!(environment["terrain"], serde_json::json!([]));
        let sheltered = !environment["objects"].as_array().unwrap().is_empty();
        habitat.insert("has_shelter".to_owned(), sheltered.into());
    }
    value["park"]
        .as_object_mut()
        .unwrap()
        .remove("next_habitat_object_id")
        .unwrap();
    content_without_environment(&mut value["content"]);
    value["schema_version"] = 8.into();
}

/// Undoes the schema 6 → 7 migration (#96) on a saved document: every habitat's animal records
/// become their count again and the park forgets its next animal ID.
fn animals_as_counts(value: &mut Value) {
    for habitat in value["park"]["habitats"].as_array_mut().unwrap() {
        let count = habitat["animals"].as_array().unwrap().len();
        habitat["animals"] = count.into();
    }
    value["park"]
        .as_object_mut()
        .unwrap()
        .remove("next_animal_id")
        .unwrap();
    // Schemas 6 and 7 embedded catalogs that still carried their own number (schema 8 drops it).
    value["content"]["schema_version"] = 3.into();
    value["schema_version"] = 6.into();
}

/// What the schema 4 → 5 migration (#95) does to a document's habitats: each rectangle becomes
/// its row-major enclosed tiles.
fn rectangles_as_tiles(value: &mut Value) {
    for habitat in value["park"]["habitats"].as_array_mut().unwrap() {
        let habitat = habitat.as_object_mut().unwrap();
        let mut take = |key: &str| habitat.remove(key).unwrap().as_u64().unwrap();
        let (x, y, width, height) = (take("x"), take("y"), take("width"), take("height"));
        habitat.remove("orientation").unwrap();
        let tiles: Vec<Value> = (y..y + height)
            .flat_map(|y| (x..x + width).map(move |x| serde_json::json!({ "x": x, "y": y })))
            .collect();
        habitat.insert("tiles".to_owned(), tiles.into());
    }
}

/// What the schema 5 → 6 migration (#95) does to a document made with the built-in content:
/// fences move into the content, every species gains its minimum fence height and every
/// habitat is fenced with the wooden rail at the old price and upkeep.
fn fences_from_content(value: &mut Value) {
    let mut built_in = json(&ZooGame::new().content_json());
    content_without_environment(&mut built_in);
    content_without_comfort(&mut built_in);
    content_without_compatibility(&mut built_in);
    content_without_health(&mut built_in);
    content_without_utilities(&mut built_in);
    content_without_education(&mut built_in);
    let wooden = built_in["fences"][0].clone();
    assert_eq!(wooden["key"], "wooden_rail");
    for habitat in value["park"]["habitats"].as_array_mut().unwrap() {
        if habitat["species"].is_object() {
            let key = habitat["species"]["key"].clone();
            let species = built_in["species"]
                .as_array()
                .unwrap()
                .iter()
                .find(|species| species["key"] == key)
                .unwrap();
            habitat["species"]["minimum_fence_height_cm"] =
                species["minimum_fence_height_cm"].clone();
        }
        habitat
            .as_object_mut()
            .unwrap()
            .insert("fence".to_owned(), wooden.clone());
    }
    value["content"] = built_in;
    value["content"]["schema_version"] = 3.into();
}

/// Schema 5 parks keep their fence price and upkeep as the wooden rail type and gain the
/// steel mesh; nothing else changes and the park runs on.
#[test]
fn the_schema_v5_fixture_moves_its_fence_into_content() {
    let fixture = std::fs::read_to_string(FIXTURE_V5).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut original = json(&fixture);
    assert_eq!(
        original["content"]["balance"]["construction"]["fence_segment_cents"],
        json(&game.content_json())["fences"][0]["segment_cents"]
    );
    fences_from_content(&mut original);
    original["schema_version"] = 6.into();
    assert_eq!(counted(&game), original, "migration changes nothing else");
    game.tick(60);
}

/// The current save of `game` with its animals counted, as schema 6 and older stored them.
fn counted(game: &ZooGame) -> Value {
    let mut value = json(&game.save_json());
    environments_as_shelters(&mut value);
    animals_as_counts(&mut value);
    value
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn ok(result: String) {
    let value = json(&result);
    assert_eq!(value["ok"], true, "{value}");
}

/// A staffed two-habitat zoo with food and drink stands, a janitor and a mechanic.
fn park() -> ZooGame {
    let mut game = ZooGame::new();
    for x in 5..=11 {
        ok(game.place_path(x, 7));
    }
    ok(game.place_habitat_rect(3, 8, 5, 10));
    ok(game.place_habitat_rect(7, 8, 9, 10));
    // Circulation (#98): a staff-only service lane between the habitats and a public plaza.
    ok(game.build_path_stroke(vec![6, 8, 6, 11], 1, "staff".to_owned()));
    ok(game.build_plaza(10, 8, 11, 9, "public".to_owned()));
    ok(game.place_concession(6, 6, "food".to_owned()));
    ok(game.place_concession(8, 6, "drink".to_owned()));
    ok(game.create_work_zone(0, 0, 19, 13));
    ok(game.buy_animal_feed());
    for keeper_id in 1..=2 {
        ok(game.hire_keeper());
        ok(game.assign_keeper_zone(keeper_id, 1));
    }
    ok(game.hire_janitor());
    ok(game.hire_mechanic());
    ok(game.set_keeper_schedule(2, "cleaning,inspection".to_owned(), 360, 1080));
    ok(game.adopt(1, "zebra".to_owned()));
    ok(game.adopt(2, "capybara".to_owned()));
    // Habitat design (#97): a shelter, a log pile and a wading pool survive the reload.
    ok(game.add_shelter(1));
    ok(game.place_habitat_object(2, "log_pile".to_owned(), 7, 10));
    ok(game.paint_habitat_terrain(2, vec![9, 9, 9, 10], "shallow_water".to_owned()));
    game
}

/// Advances to a mid-run point where guests walk, care is under way and stands are worn.
fn mid_run() -> ZooGame {
    let mut game = park();
    for _ in 0..13 {
        game.tick(37);
    }
    let view = json(&game.snapshot_json());
    let guests = view["guests"].as_array().unwrap();
    assert!(
        guests.iter().any(|guest| guest["state"] != "viewing"),
        "guests are traveling"
    );
    let jobs: Vec<&Value> = view["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|habitat| habitat["care_jobs"].as_array().unwrap())
        .collect();
    assert!(!jobs.is_empty(), "care jobs exist");
    assert!(
        view["concessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stand| stand["condition"].as_u64().unwrap() < 100),
        "facility wear"
    );
    game
}

/// Identical commands and ticks after the save point, crossing into the next finance day.
fn continue_run(game: &mut ZooGame) {
    ok(game.refill_water(2));
    for _ in 0..30 {
        game.tick(29);
    }
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(3, 1));
    ok(game.place_concession(10, 6, "food".to_owned()));
    for _ in 0..20 {
        game.tick(31);
    }
}

#[test]
fn a_reloaded_park_continues_exactly_like_an_uninterrupted_one() {
    let mut continuous = mid_run();
    let day_at_save = json(&continuous.snapshot_json())["day"].clone();

    let saved = mid_run().save_json();
    let mut resumed = ZooGame::new();
    ok(resumed.load_save(&saved));
    assert_eq!(resumed.save_json(), saved, "load then save is lossless");
    // Construction undo history is session state (#92): a load starts with none.
    let without_history = |game: &ZooGame| {
        let mut view = json(&game.snapshot_json());
        let history = view
            .as_object_mut()
            .unwrap()
            .remove("construction")
            .unwrap();
        (view, history)
    };
    let (resumed_view, resumed_history) = without_history(&resumed);
    let (continuous_view, continuous_history) = without_history(&continuous);
    assert_eq!(resumed_view, continuous_view);
    assert_eq!(resumed_history["undo_depth"], 0);
    assert_ne!(continuous_history["undo_depth"], 0);

    continue_run(&mut continuous);
    continue_run(&mut resumed);
    assert_ne!(json(&continuous.snapshot_json())["day"], day_at_save);
    assert_eq!(resumed.save_json(), continuous.save_json());
    assert_eq!(resumed.snapshot_json(), continuous.snapshot_json());
}

#[test]
fn rejected_saves_leave_the_current_park_untouched() {
    let saved = mid_run().save_json();
    let mut game = park();
    game.tick(45);
    let before = game.save_json();
    let snapshot_before = game.snapshot_json();

    let mutate = |edit: &dyn Fn(&mut Value)| {
        let mut value = json(&saved);
        edit(&mut value);
        value.to_string()
    };
    let cases = [
        ("malformed", saved[..saved.len() / 2].to_owned()),
        ("malformed", "not json".to_owned()),
        ("too_large", " ".repeat(4 * 1024 * 1024 + 1)),
        (
            "unsupported_format",
            mutate(&|v| v["format"] = "zoo-diagnostics/1".into()),
        ),
        (
            "unsupported_version",
            mutate(&|v| v["schema_version"] = 1_000.into()),
        ),
        (
            "invalid_save",
            mutate(&|v| v["park"]["cash_cents"] = "lots".into()),
        ),
        (
            "invalid_save",
            mutate(&|v| v["park"]["surprise"] = true.into()),
        ),
        (
            "invalid_content",
            mutate(&|v| v["content"]["species"][0]["asset_ref"] = "missing".into()),
        ),
        (
            "invalid_park",
            mutate(&|v| {
                let keeper = v["park"]["keepers"][0].clone();
                v["park"]["keepers"].as_array_mut().unwrap().push(keeper);
            }),
        ),
        (
            "invalid_park",
            mutate(&|v| v["park"]["guests"][0]["target_habitat"] = 99.into()),
        ),
        (
            "invalid_park",
            mutate(&|v| v["park"]["habitats"][0]["residents"][0]["welfare"] = 101.into()),
        ),
        (
            "invalid_park",
            mutate(&|v| {
                v["park"]["guests"][0]["x"] = 6.into();
                v["park"]["guests"][0]["y"] = 9.into();
            }),
        ),
        (
            "invalid_park",
            mutate(&|v| v["park"]["next_keeper_id"] = 1.into()),
        ),
    ];
    for (code, document) in cases {
        let result = json(&game.load_save(&document));
        assert_eq!(result["ok"], false, "{code}: {result}");
        assert_eq!(result["code"], code, "{result}");
        assert_eq!(game.save_json(), before);
        assert_eq!(game.snapshot_json(), snapshot_before);
        assert_eq!(game.session_status(), "running");
    }
}

#[test]
fn repeated_imports_replace_rather_than_accumulate() {
    let saved = mid_run().save_json();
    let mut game = ZooGame::new();
    ok(game.load_save(&saved));
    ok(game.load_save(&saved));
    ok(game.load_save(&saved));
    assert_eq!(game.save_json(), saved);
}

/// Schema 6 animal counts become individual records (#96): park-wide IDs in habitat then slot
/// order, every animal of its habitat's species standing inside it, and nothing else changes.
#[test]
fn the_schema_v6_fixture_migrates_counts_to_individual_animals() {
    let fixture = std::fs::read_to_string(FIXTURE_V6).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let original = json(&fixture);
    let saved = json(&game.save_json());
    assert_eq!(counted(&game), original, "migration changes nothing else");

    let mut expected_id = 1;
    for (habitat, before) in saved["park"]["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .zip(original["park"]["habitats"].as_array().unwrap())
    {
        let animals = habitat["animals"].as_array().unwrap();
        assert_eq!(animals.len() as u64, before["animals"].as_u64().unwrap());
        for animal in animals {
            assert_eq!(animal["id"], expected_id);
            assert_eq!(animal["species"], habitat["residents"][0]["species"]["key"]);
            assert!(
                habitat["tiles"]
                    .as_array()
                    .unwrap()
                    .contains(&animal["position"]),
                "{animal}"
            );
            expected_id += 1;
        }
    }
    assert!(expected_id > 2, "the fixture keeps several animals");
    assert_eq!(saved["park"]["next_animal_id"], expected_id);
    game.tick(60);
}

/// Schema 7 catalogs lose their own version number; nothing else changes.
#[test]
fn the_schema_v7_fixture_migrates_by_dropping_the_content_version() {
    let fixture = std::fs::read_to_string(FIXTURE_V7).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut original = json(&fixture);
    original["content"]
        .as_object_mut()
        .unwrap()
        .remove("schema_version")
        .unwrap();
    original["schema_version"] = 8.into();
    let mut saved = json(&game.save_json());
    environments_as_shelters(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}

/// Schema 8 shelters become basic shelter objects (#97) on each habitat's default tile, the
/// content gains environment prices and requirements, and nothing else changes.
#[test]
fn the_schema_v8_fixture_migrates_shelters_to_habitat_objects() {
    let mut original = json(&std::fs::read_to_string(FIXTURE_V8).unwrap());
    // The recorded fixture predates shelters in its run; give its first habitat one.
    original["park"]["habitats"][0]["has_shelter"] = true.into();
    let mut game = ZooGame::new();
    ok(game.load_save(&original.to_string()));
    let mut saved = json(&game.save_json());
    let mut next_id = 1;
    for (habitat, before) in saved["park"]["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .zip(original["park"]["habitats"].as_array().unwrap())
    {
        let objects = habitat["environment"]["objects"].as_array().unwrap();
        assert_eq!(objects.len(), usize::from(before["has_shelter"] == true));
        for object in objects {
            assert_eq!(object["id"], next_id);
            assert_eq!(object["definition"]["key"], "basic_shelter");
            next_id += 1;
        }
    }
    assert!(next_id > 1, "the fixture has a sheltered habitat");
    assert_eq!(saved["park"]["next_habitat_object_id"], next_id);
    environments_as_shelters(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}

/// Schema 9 path tiles all become public paths (#98); nothing else changes.
#[test]
fn the_schema_v9_fixture_migrates_every_path_to_public() {
    let fixture = std::fs::read_to_string(FIXTURE_V9).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    let public = saved["park"]["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tile| **tile == serde_json::json!({ "path": "public" }))
        .count();
    assert!(public > 0, "the fixture has paths");
    paths_unclassified(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}

#[test]
fn the_schema_v10_fixture_migrates_to_a_park_that_owns_its_whole_map() {
    let fixture = std::fs::read_to_string(FIXTURE_V10).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    land_removed(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    let view = json(&game.snapshot_json());
    assert_eq!(view["land"]["parcels"], serde_json::json!([]));
    assert_eq!(view["land"]["owned_tiles"], view["land"]["total_tiles"]);
    // Saved path classifications restore (#98): a staff-only route and a widened plaza.
    let view = json(&game.snapshot_json());
    let access = |x: u64, y: u64| {
        view["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tile| tile["x"] == x && tile["y"] == y)
            .unwrap()["access"]
            .clone()
    };
    assert_eq!(access(6, 9), "staff");
    assert_eq!(access(11, 9), "public");
    let diagnostics = json(&game.diagnostics_json());
    assert_eq!(diagnostics["start"]["scenario"], "save");
    game.tick(60);
}

/// The partly-prepared lakeside after buying the East Paddock (#99), with a path into it and a
/// second, stocked and staffed habitat on the new land.
fn expanded() -> ZooGame {
    let mut game = ZooGame::new();
    ok(game.start_scenario("partly-prepared"));
    let parcels = vec!["east-paddock".to_owned()];
    let quote = json(&game.evaluate_land_purchase(parcels.clone()));
    ok(game.buy_land(parcels, quote["quote"].as_str().unwrap().to_owned()));
    ok(game.build_path_stroke(vec![12, 8, 22, 8], 1, "public".to_owned()));
    ok(game.place_habitat_rect(19, 9, 22, 12));
    ok(game.create_work_zone(3, 6, 23, 10));
    ok(game.hire_keeper());
    ok(game.assign_keeper_zone(1, 1));
    ok(game.buy_animal_feed());
    ok(game.adopt(2, "capybara".to_owned()));
    ok(game.adopt(2, "capybara".to_owned()));
    for _ in 0..6 {
        game.tick(37);
    }
    game
}

#[test]
fn the_schema_v11_fixture_migrates_to_a_park_that_can_build_comfort_facilities() {
    let fixture = std::fs::read_to_string(FIXTURE_V11).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    comfort_removed(&mut saved);
    assert_eq!(saved, json(&fixture), "migration changes nothing else");
    // The migrated catalog offers the comfort facilities, so its guests' new need can be met.
    let view = json(&game.snapshot_json());
    let offered: Vec<&str> = view["facility_catalog"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|facility| facility["key"].as_str())
        .collect();
    for key in COMFORT_FACILITIES {
        assert!(offered.contains(&key), "{offered:?}");
    }
    ok(game.place_concession(8, 7, "toilet".to_owned()));
    // Saved land restores (#99): the bought paddock stays owned and the rest stays for sale.
    let view = json(&game.snapshot_json());
    let status = |id: &str| {
        view["land"]["parcels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|parcel| parcel["id"] == id)
            .unwrap()["status"]
            .clone()
    };
    assert_eq!(status("east-paddock"), "owned");
    assert_eq!(status("lakeshore"), "available");
    assert_eq!(view["habitats"].as_array().unwrap().len(), 2);
    let built = json(&game.evaluate_path_stroke(vec![18, 7], 1, "public".to_owned()));
    assert_eq!(built["ok"], false);
    assert_eq!(built["land_parcels"], serde_json::json!(["lakeshore"]));
    game.tick(60);
}

/// `expanded` with toilets, a bench and a litter bin by the main path and a janitor, run until
/// guests hold toilet slots and a facility has asked for a janitor (#101).
fn comforted() -> ZooGame {
    let mut game = expanded();
    ok(game.place_concession(8, 7, "toilet".to_owned()));
    ok(game.place_concession(10, 9, "bench".to_owned()));
    ok(game.place_concession(6, 9, "bin".to_owned()));
    ok(game.hire_janitor());
    for _ in 0..400 {
        game.tick(1);
        let view = json(&game.snapshot_json());
        let errand = view["guests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guest| guest["state"] == "using_facility");
        if errand && !view["facility_service"].as_array().unwrap().is_empty() {
            return game;
        }
    }
    panic!("no guest used the toilets while a facility awaited service");
}

/// Schema 12 parks keep every errand, slot and service task (#101); their stands open with one
/// vendor and full stock, guests gain their spending money, and the park runs on and sells.
#[test]
fn the_schema_v12_fixture_migrates_to_staffed_stands() {
    let fixture = std::fs::read_to_string(FIXTURE_V12).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    staffed_stands_removed(&mut saved);
    assert_eq!(saved, json(&fixture), "migration changes nothing else");
    let view = json(&game.snapshot_json());
    let food = view["concessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stand| stand["kind"] == "food")
        .unwrap()
        .clone();
    assert_eq!(food["stand"]["vendors"], 1);
    assert_eq!(food["stand"]["stock"], food["stand"]["stock_capacity"]);
    let sold = food["total_sales"].as_u64().unwrap();
    // The worn stand needs a mechanic to keep selling.
    ok(game.hire_mechanic());
    for _ in 0..4 {
        game.tick(60);
    }
    assert_eq!(game.session_status(), "running");
    let view = json(&game.snapshot_json());
    let food = view["concessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stand| stand["kind"] == "food")
        .unwrap();
    assert!(food["total_sales"].as_u64().unwrap() > sold, "{food}");
}

/// `comforted` with a drink stand beside the food stand and two vendors at the food stand, run
/// until both a guest is being served and another queues at a stand, with a delivery on order
/// (#102).
fn staffed() -> ZooGame {
    let mut game = comforted();
    ok(game.place_concession(7, 9, "drink".to_owned()));
    ok(game.set_stand_vendors(1, 2));
    for _ in 0..600 {
        game.tick(1);
        let save = json(&game.save_json());
        let stands = save["park"]["concessions"].as_array().unwrap().clone();
        let serving = stands.iter().any(|stand| {
            stand["stand"].is_object() && !stand["slots"].as_array().unwrap().is_empty()
        });
        let queueing = stands.iter().any(|stand| {
            stand["stand"]["queue"]
                .as_array()
                .is_some_and(|queue| !queue.is_empty())
        });
        // A group of several people is in the park (#103).
        let grouped = save["park"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| group["members"].as_array().unwrap().len() > 1);
        let serviced = !save["park"]["facility_service"]
            .as_array()
            .unwrap()
            .is_empty();
        if serving && queueing && grouped && serviced {
            ok(game.restock_stand(1));
            return game;
        }
    }
    panic!("no stand served one guest while another queued");
}

/// Schema 13 parks keep every guest, errand, reservation and queue; each guest becomes its own
/// group (#103), and the park runs on and admits new groups.
#[test]
fn the_schema_v13_fixture_migrates_to_visitor_groups() {
    let fixture = std::fs::read_to_string(FIXTURE_V13).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    groups_removed(&mut saved);
    assert_eq!(saved, json(&fixture), "migration changes nothing else");
    let before = json(&game.snapshot_json())["visits"]["groups_arrived"]
        .as_u64()
        .unwrap();
    game.tick(180);
    assert_eq!(game.session_status(), "running");
    let view = json(&game.snapshot_json());
    assert!(view["visits"]["groups_arrived"].as_u64().unwrap() > before);
    assert!(
        view["visits"]["total"]
            .as_object()
            .unwrap()
            .values()
            .any(|n| n.as_u64() > Some(0))
    );
}

/// The staffed park run on until a keeper carries a collected feed crate (#104).
fn cared() -> ZooGame {
    let mut game = staffed();
    for _ in 0..600 {
        let save = json(&game.save_json());
        let carrying = save["park"]["keepers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|keeper| keeper["carrying_feed"] == true);
        if carrying {
            return game;
        }
        game.tick(1);
    }
    panic!("no keeper ever carried a feed crate");
}

/// Schema 14 parks took supplies at dispatch (#104): the in-progress feeding's crate is in its
/// worker's hands, earlier reserved cleanings were prepaid, and the park runs on, feeding
/// with the crate it already carries rather than taking another.
#[test]
fn the_schema_v14_fixture_migrates_to_physical_keeper_supplies() {
    let fixture = std::fs::read_to_string(FIXTURE_V14).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    let park = &saved["park"];
    let carried = park["care_jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["state"] == "in_progress")
        .unwrap();
    assert_eq!(carried["supply"], "feed_carried");
    assert_eq!(park["keepers"][0]["carrying_feed"], true);
    let shelf = park["feed_crates"].as_u64().unwrap();
    physical_care_removed(&mut saved);
    assert_eq!(saved, json(&fixture), "migration changes nothing else");

    // The carried crate is fed: the shelf is untouched by that job and the keeper's hands
    // empty once it completes.
    game.tick(5);
    let view = json(&game.snapshot_json());
    let keeper = &view["animal_care_depot"]["keepers"][0];
    assert_eq!(keeper["carrying_feed"], false, "{keeper}");
    assert!(view["animal_care_depot"]["feed_crates"].as_u64().unwrap() <= shelf);
    game.tick(180);
    assert_eq!(game.session_status(), "running");
}

/// Schema 15 parks had one price per stand and the catalog's admission (#106): they open at
/// exactly those prices, a guest already at a window keeps the price it agreed, and the park
/// runs on, admitting the next group at the catalog price out of its seeded budget.
#[test]
fn the_schema_v15_fixture_migrates_to_player_prices() {
    let fixture = std::fs::read_to_string(FIXTURE_V15).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    pricing_removed(&mut saved);
    assert_eq!(saved, json(&fixture), "migration changes nothing else");

    let view = json(&game.snapshot_json());
    assert_eq!(view["pricing"]["admission_price_cents"], 1200);
    assert_eq!(view["pricing"]["open"], true);
    let arrived = view["guest_count"].as_u64().unwrap();
    let mut admitted = 0;
    for _ in 0..240 {
        game.tick(1);
        admitted = json(&game.snapshot_json())["pricing"]["total"]["people_admitted"]
            .as_u64()
            .unwrap();
        if admitted > 0 {
            break;
        }
    }
    assert!(
        admitted > 0,
        "nobody was admitted after loading ({arrived} inside)"
    );
    let view = json(&game.snapshot_json());
    assert_eq!(
        view["pricing"]["total"]["admission_cents"]
            .as_i64()
            .unwrap(),
        1200 * admitted as i64
    );
    assert_eq!(game.session_status(), "running");
}

/// The cared-for park with player prices (#106): a dearer ticket, a running campaign, and a
/// stand repriced while a guest it is serving keeps the price it agreed.
fn priced() -> ZooGame {
    let mut game = cared();
    ok(game.set_admission_price(1_500.0));
    ok(game.start_marketing_campaign());
    for _ in 0..600 {
        let save = json(&game.save_json());
        let serving = save["park"]["guests"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|guest| {
                let visit = &guest["visit"];
                (guest["state"] == "using_facility" && visit["price_cents"].as_i64() > Some(0))
                    .then(|| {
                        (
                            visit["facility_id"].as_u64().unwrap(),
                            visit["price_cents"].as_i64().unwrap(),
                        )
                    })
            });
        if let Some((stand, price)) = serving {
            ok(game.set_stand_price(stand as u32, (price + 100) as f64));
            return game;
        }
        game.tick(1);
    }
    panic!("no guest was ever served at a stand");
}

/// Schema 16 parks kept one species per habitat (#108): each becomes that habitat's only
/// resident with exactly its welfare, the catalog gains the built-in mixed-exhibit profiles,
/// and the park runs on.
#[test]
fn the_schema_v16_fixture_migrates_to_single_species_residents() {
    let fixture = std::fs::read_to_string(FIXTURE_V16).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    assert_eq!(
        saved["content"]["species_compatibility"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "the built-in profiles join the park's catalog"
    );
    residents_removed(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");

    let view = json(&game.snapshot_json());
    for (habitat, before) in view["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .zip(original["park"]["habitats"].as_array().unwrap())
    {
        assert_eq!(habitat["welfare"], before["welfare"]);
        let species: Vec<Value> = before["species"].get("key").into_iter().cloned().collect();
        assert_eq!(habitat["species"], Value::Array(species));
    }
    game.tick(120);
    assert_eq!(game.session_status(), "running");
}

/// The priced park with a mixed exhibit (#108): three zebras and two giraffes, each group
/// adopted at once, share habitat #1's steel-mesh yard.
fn mixed() -> ZooGame {
    let mut game = priced();
    ok(game.upgrade_fence(1, "steel_mesh".to_owned()));
    ok(game.adopt_group(1, "zebra".to_owned(), 3));
    ok(game.adopt_group(1, "giraffe".to_owned(), 2));
    game
}

/// Schema 17 parks knew nothing of health (#109): every animal becomes healthy with no risk,
/// every worker a keeper, the catalog gains the vet and medicine prices, and the park runs on.
#[test]
fn the_schema_v17_fixture_migrates_to_healthy_animals_and_keepers() {
    let fixture = std::fs::read_to_string(FIXTURE_V17).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    assert_eq!(
        saved["content"]["balance"]["staff"]["vet"]["hire_cents"],
        30_000
    );
    health_removed(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    let view = json(&game.snapshot_json());
    assert!(
        view["animals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|animal| animal["health"]["status"] == "healthy")
    );
    game.tick(120);
    assert_eq!(game.session_status(), "running");
}

/// The mixed park with veterinary cover (#109): a vet hired and medicine on the shelf.
fn vetted() -> ZooGame {
    let mut game = mixed();
    ok(game.hire_vet());
    ok(game.buy_medicine());
    game
}

/// Schema 18 parks knew nothing of utilities (#113): every habitat gets normal priority and
/// nothing to supply, the catalog gains the two plants, and the park runs on and can build one.
#[test]
fn the_schema_v18_fixture_migrates_to_parks_without_utility_demand() {
    let fixture = std::fs::read_to_string(FIXTURE_V18).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    utilities_removed(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    let view = json(&game.snapshot_json());
    assert!(view["habitats"].as_array().unwrap().iter().all(
        |habitat| habitat["utilities"]["water"]["status"] == "not_needed"
            && habitat["utilities"]["climate"]["status"] == "not_needed"
    ));
    game.tick(120);
    assert_eq!(game.session_status(), "running");
    let (x, y) = beside_path(&game);
    ok(game.place_concession(x, y, "water_pump".to_owned()));
}

/// The first grass tile beside a path, row by row.
fn beside_path(game: &ZooGame) -> (u32, u32) {
    let view = json(&game.snapshot_json());
    let tiles = view["tiles"].as_array().unwrap();
    let width = view["width"].as_u64().unwrap() as i64;
    let height = view["height"].as_u64().unwrap() as i64;
    let kind = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= width || y >= height {
            return "";
        }
        tiles[(y * width + x) as usize]["kind"].as_str().unwrap()
    };
    for y in 0..height {
        for x in 0..width {
            if kind(x, y) == "grass"
                && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| kind(x + dx, y + dy) == "path")
                && parse_ok(&game.evaluate_concession(x as u32, y as u32, "water_pump".to_owned()))
            {
                return (x as u32, y as u32);
            }
        }
    }
    panic!("no grass beside a path");
}

fn parse_ok(preview: &str) -> bool {
    json(preview)["ok"] == true
}

/// The vetted park with an idle water pump (#113).
fn utilized() -> ZooGame {
    let mut game = vetted();
    let (x, y) = beside_path(&game);
    ok(game.place_concession(x, y, "water_pump".to_owned()));
    game
}

/// The utilities park with a loan partly repaid (#115): $5,000 borrowed and $1,000 repaid.
fn indebted() -> ZooGame {
    let mut game = utilized();
    ok(game.borrow_loan(500_000.0));
    ok(game.repay_loan(100_000.0));
    game
}

/// Schema 19 parks could not borrow (#115): they gain their built-in scenario's credit line
/// with nothing owed and no deficit days, and their previous day becomes the history's one day
/// with exactly the cash the ledgers explain.
#[test]
fn the_schema_v19_fixture_migrates_to_parks_with_a_credit_line_and_history() {
    let fixture = std::fs::read_to_string(FIXTURE_V19).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    finances_removed(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    let view = json(&game.snapshot_json());
    assert_eq!(view["finance"]["loan"]["available"], true);
    assert_eq!(view["finance"]["loan"]["balance_cents"], 0);
    assert!(view["finance"]["history"].as_array().unwrap().is_empty());
    ok(game.borrow_loan(10_000.0));

    // A park on day 2 with a previous day: the day is reconstructed and reconciles.
    let mut second = original.clone();
    second["park"]["day"] = 2.into();
    let mut previous = second["park"]["finance_today"].clone();
    previous["admissions_income_cents"] = 30_000.into();
    previous["keeper_wages_expense_cents"] = 12_000.into();
    second["park"]["finance_previous"] = previous;
    let mut game = ZooGame::new();
    ok(game.load_save(&second.to_string()));
    let view = json(&game.snapshot_json());
    let today = &view["finance"]["current_day"];
    let day = &view["finance"]["history"][0];
    assert_eq!(day["day"], 1);
    assert_eq!(day["closing_cash_cents"], today["opening_cash_cents"]);
    assert_eq!(
        day["opening_cash_cents"].as_i64().unwrap() + day["net_cash_flow_cents"].as_i64().unwrap(),
        day["closing_cash_cents"].as_i64().unwrap()
    );
    assert_eq!(
        today["opening_cash_cents"].as_i64().unwrap()
            + today["net_cash_flow_cents"].as_i64().unwrap(),
        view["cash_cents"].as_i64().unwrap()
    );
    let mut saved = json(&game.save_json());
    finances_removed(&mut saved);
    // Loading re-reads the alerts at the edited clock; everything else is as saved.
    second["park"]["alerts"] = saved["park"]["alerts"].clone();
    assert_eq!(saved, second, "migration changes nothing else");
}

/// Schema 20 parks taught nobody (#116): they gain an empty education record, guests without
/// lessons, zero donation and grant lines, and the built-in education sign and facts.
#[test]
fn the_schema_v20_fixture_migrates_to_parks_that_can_teach() {
    let fixture = std::fs::read_to_string(FIXTURE_V20).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    assert!(
        saved["content"]["facts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|fact| fact["source_url"].as_str().unwrap().starts_with("https://"))
    );
    education_removed(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    let view = json(&game.snapshot_json());
    assert!(view["education"]["talks"].as_array().unwrap().is_empty());
    assert!(
        view["facility_catalog"]
            .as_array()
            .unwrap()
            .iter()
            .any(|facility| facility["service"] == "sign")
    );
    game.tick(60);
}

/// The indebted park teaching (#116): a sign beside the path, a daily talk, guests who learned
/// and places reserved at an open talk.
fn educated() -> ZooGame {
    let mut game = indebted();
    let (x, y) = beside_path(&game);
    ok(game.place_concession(x, y, "sign".to_owned()));
    let minute = json(&game.snapshot_json())["minute_of_day"]
        .as_u64()
        .unwrap() as u32;
    ok(game.schedule_talk(1, (minute + 15).div_ceil(15) * 15 % 1440, 30, 6));
    game
}

/// Schema 21 parks always had fair weather (#118): they gain a calendar without a weather
/// stream, both rules on and nothing announced, and stay fair however long they run.
#[test]
fn the_schema_v21_fixture_migrates_to_parks_with_fair_weather() {
    let fixture = std::fs::read_to_string(FIXTURE_V21).unwrap();
    let original = json(&fixture);
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = json(&game.save_json());
    conditions_removed(&mut saved);
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(240);
    let view = json(&game.snapshot_json());
    let conditions = &view["conditions"];
    assert_eq!(conditions["weather_stream"], false);
    assert!(conditions["scheduled"].is_null());
    assert!(
        conditions["forecast"]
            .as_array()
            .unwrap()
            .iter()
            .all(|day| day["weather"] == "fair")
    );
}

/// The educated park with park events switched off (#118).
fn current_fixture_park() -> ZooGame {
    let mut game = educated();
    ok(game.set_condition_rule("events".to_owned(), false));
    game
}

#[test]
fn the_current_schema_fixture_still_loads_unchanged() {
    if std::env::var_os("ZOO_WRITE_SAVE_FIXTURE").is_some() {
        let value = json(&current_fixture_park().save_json());
        std::fs::write(
            FIXTURE_V22,
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
    let fixture = std::fs::read_to_string(FIXTURE_V22).unwrap();
    let saved = json(&fixture);
    assert_eq!(
        saved["park"]["conditions"]["rules"]["events"], false,
        "the fixture switched park events off"
    );
    assert!(
        !saved["park"]["education"]["talks"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the fixture schedules a talk"
    );
    assert_eq!(
        saved["park"]["finance"]["loan"]["balance_cents"], 400_000,
        "the fixture owes a loan"
    );
    let saved = json(&fixture);
    assert!(
        saved["park"]["keepers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|worker| worker["role"] == "vet"),
        "the fixture holds a vet"
    );
    assert_eq!(
        saved["park"]["medicine_kits"], 5,
        "the fixture holds medicine"
    );
    let residents = saved["park"]["habitats"][0]["residents"]
        .as_array()
        .unwrap();
    assert_eq!(
        residents
            .iter()
            .map(|resident| resident["species"]["key"].clone())
            .collect::<Vec<_>>(),
        ["zebra", "giraffe"],
        "the fixture holds a mixed exhibit"
    );
    let pricing = &saved["park"]["pricing"];
    assert_eq!(
        pricing["admission_cents"], 1_500,
        "the fixture holds a player price"
    );
    assert!(
        pricing["campaign"].is_object(),
        "the fixture holds a running campaign"
    );
    let agreed = saved["park"]["guests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|guest| {
            guest["state"] == "using_facility" && guest["visit"]["price_cents"].as_i64() > Some(0)
        })
        .expect("the fixture holds a guest at a stand window");
    let stand = saved["park"]["concessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stand| stand["id"] == agreed["visit"]["facility_id"])
        .unwrap();
    assert_eq!(
        stand["stand"]["price_cents"].as_i64().unwrap(),
        agreed["visit"]["price_cents"].as_i64().unwrap() + 100,
        "the fixture holds a sale agreed before a price change"
    );
    assert!(
        json(&fixture)["park"]["keepers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|keeper| keeper["carrying_feed"] == true),
        "the fixture holds a carried feed crate"
    );
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    assert_eq!(json(&game.save_json()), json(&fixture));
    // Saved reservations, errands, queues, stand windows, deliveries and service work restore
    // (#101, #102) and run on like an uninterrupted park.
    let mut uninterrupted = ZooGame::new();
    ok(uninterrupted.load_save(&fixture));
    let park = json(&fixture)["park"].clone();
    let stands = park["concessions"].as_array().unwrap();
    let slots: usize = stands
        .iter()
        .map(|stand| stand["slots"].as_array().unwrap().len())
        .sum();
    assert!(slots > 0, "the fixture holds a reserved or used slot");
    assert!(
        stands.iter().any(|stand| stand["stand"]["queue"]
            .as_array()
            .is_some_and(|q| !q.is_empty())),
        "the fixture holds a stand queue"
    );
    assert!(
        stands
            .iter()
            .any(|stand| stand["stand"]["delivery_due"].is_u64()),
        "the fixture holds a delivery on order"
    );
    assert!(!park["facility_service"].as_array().unwrap().is_empty());
    ok(game.load_save(&game.save_json()));
    game.tick(90);
    uninterrupted.tick(90);
    assert_eq!(game.save_json(), uninterrupted.save_json());
}

/// Schema 3 parks gain alerts, re-read from the park as it loads; nothing else changes.
#[test]
fn the_schema_v3_fixture_migrates_by_reading_its_alerts() {
    let fixture = std::fs::read_to_string(FIXTURE_V3).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = counted(&game);
    let alerts = saved["park"]
        .as_object_mut()
        .unwrap()
        .remove("alerts")
        .unwrap();
    let minute = json(&game.snapshot_json());
    let now =
        (minute["day"].as_u64().unwrap() - 1) * 1440 + minute["minute_of_day"].as_u64().unwrap();
    for alert in alerts.as_array().unwrap() {
        assert_eq!(alert["first_minute"].as_u64(), Some(now), "{alert}");
    }
    saved["schema_version"] = 3.into();
    let mut original = json(&fixture);
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}

/// Schema 4 rectangles become enclosure tiles (#95) without changing area, capacity, care,
/// gates or ownership, and the park runs on exactly as before.
#[test]
fn the_schema_v4_fixture_migrates_rectangles_to_enclosure_tiles() {
    let fixture = std::fs::read_to_string(FIXTURE_V4).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut original = json(&fixture);
    let rectangles: Vec<(u64, u64)> = original["park"]["habitats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            let area = h["width"].as_u64().unwrap() * h["height"].as_u64().unwrap();
            (h["id"].as_u64().unwrap(), area)
        })
        .collect();
    assert!(!rectangles.is_empty());
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
    original["schema_version"] = 6.into();
    assert_eq!(counted(&game), original, "migration changes nothing else");

    let view = json(&game.snapshot_json());
    for (id, area) in rectangles {
        let habitat = view["habitats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["id"] == id)
            .unwrap();
        assert_eq!(habitat["footprint_area"], area);
        assert_eq!(habitat["capacity"], (area / 3).clamp(2, 12));
    }
    game.tick(60);
}

/// Schema 2 keeper-training parks continue as the guided opening scenario: same park, same
/// guide status, the opening objectives re-checked from park state on the next command.
#[test]
fn the_schema_v2_fixture_migrates_keeper_training_to_the_opening_guide() {
    let fixture = std::fs::read_to_string(FIXTURE_V2).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = counted(&game);
    let park = saved["park"].as_object_mut().unwrap();
    assert_eq!(park["scenario"]["id"], "first-zoo");
    assert_eq!(park["tutorial"], serde_json::json!({ "status": "active" }));
    assert_eq!(park["objectives"].as_array().unwrap().len(), 13);
    let mut original = json(&fixture);
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
    for value in [&mut saved, &mut original] {
        let park = value["park"].as_object_mut().unwrap();
        for key in ["tutorial", "objectives", "alerts"] {
            park.remove(key);
        }
        park["scenario"].as_object_mut().unwrap().remove("id");
        value["schema_version"] = 0.into();
    }
    assert_eq!(saved, original, "migration changes nothing else");

    ok(game.buy_animal_feed());
    let view = json(&game.snapshot_json());
    let done = view["scenario"]["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|objective| objective["done"] == true)
        .count();
    assert!(done >= 5, "the fixture's built zoo ticks its steps: {view}");
    assert_eq!(view["tutorial"]["status"], "active");
}

/// Schema 1 parks were all the fixed empty starter layout; migration records exactly that.
#[test]
fn the_schema_v1_fixture_migrates_to_the_opening_scenario() {
    let fixture = std::fs::read_to_string(FIXTURE_V1).unwrap();
    let mut game = ZooGame::new();
    ok(game.load_save(&fixture));
    let mut saved = counted(&game);
    let park = saved["park"].as_object_mut().unwrap();
    assert_eq!(park["scenario"]["id"], "first-zoo");
    assert_eq!(park["scenario"]["digest"], Value::Null);
    assert_eq!(park["entrance"], serde_json::json!({ "x": 0, "y": 7 }));
    assert_eq!(park["depot"], serde_json::json!({ "x": 2, "y": 6 }));
    for key in [
        "entrance",
        "depot",
        "scenario",
        "unlocks",
        "objectives",
        "tutorial",
        "alerts",
    ] {
        park.remove(key);
    }
    saved["schema_version"] = 1.into();
    let mut original = json(&fixture);
    rectangles_as_tiles(&mut original);
    fences_from_content(&mut original);
    original["park"].as_object_mut().unwrap().remove("tutorial");
    assert_eq!(saved, original, "migration changes nothing else");
    game.tick(60);
}
