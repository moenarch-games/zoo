//! Versioned park saves (#88).
//!
//! A save is one JSON envelope owned by `zoo-core`:
//!
//! ```json
//! { "format": "zoo-save", "schema_version": 1, "content": { ...catalog... }, "park": { ... } }
//! ```
//!
//! `content` is the validated catalog the park runs on (it carries the ruleset id), so a save
//! restores the same rules and species definitions it was made with.
//! `park` is the complete authoritative [`GameState`]: tiles, entities, stable next IDs, tick
//! and time accumulators, stocks, ledgers, care jobs, staff assignments and tutorial progress.
//! The simulation uses no random streams. Camera, windows and device settings are never part of
//! a save; neither are the session recorder, pending domain events or diagnostic work counters.
//! Loading resumes at the saved simulation time and never simulates elapsed wall-clock time.
//!
//! Loading parses, migrates, deserializes and validates the whole document before it replaces
//! the active park; any failure returns a structured error and leaves the current park as it
//! was. Because a load replaces the park rather than merging into it, repeated imports cannot
//! duplicate animals, staff, money, tasks or admissions.
//!
//! # Migration procedure
//!
//! Every change to the saved shape (including the embedded catalog's shape) appends one
//! function to [`MIGRATIONS`] that rewrites the previous schema's raw JSON into the new one,
//! and one immutable fixture `tests/fixtures/save-v<n>.json` that must keep loading. Never
//! write a schema number by hand: [`SCHEMA_VERSION`] is the length of the migration list plus
//! one, and [`migrate`] stamps it once at the end (docs/decisions/merge-safe-format-identities.md).
//! Migrations run in order on the raw JSON value before typed deserialization, so the typed
//! `GameState` only ever reads the current schema. Documents newer than this build are
//! rejected as `unsupported_version`.
use super::{
    GameState, GuestState, MAX_GUEST_HABITATS_PER_VISIT, PathAccess, Position, TileKind, animal,
    care::CareJobState,
    content::{self, Catalog},
    environment, scenario, shape,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) const FORMAT: &str = "zoo-save";
type Migration = fn(Value) -> Result<Value, SaveError>;
/// Append-only: entry `i` rewrites a schema `i + 1` document into schema `i + 2`.
const MIGRATIONS: &[Migration] = &[
    v1_to_v2,
    v2_to_v3,
    v3_to_v4,
    v4_to_v5,
    v5_to_v6,
    v6_to_v7,
    drop_content_schema_version,
    v8_add_habitat_environments,
    v9_classify_paths,
];
/// Derived, never hand-written: two PRs that each add a migration both append after the same
/// last entry of [`MIGRATIONS`], so git refuses to merge them silently.
pub(crate) const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32 + 1;
/// Far above any reachable park on the fixed grid; rejects oversized documents before parsing.
pub(crate) const MAX_SAVE_BYTES: usize = 4 * 1024 * 1024;
const MINUTES_PER_DAY: u32 = 24 * 60;

/// A structured load failure. `code` is stable for callers; `message` explains it.
#[derive(Debug, Serialize)]
pub(crate) struct SaveError {
    pub code: &'static str,
    pub message: String,
}

impl SaveError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Serializes the park and its content into a current-schema save document.
pub(crate) fn encode(state: &GameState) -> String {
    let envelope = json!({
        "format": FORMAT,
        "schema_version": SCHEMA_VERSION,
        "content": state.catalog.as_ref(),
        "park": state,
    });
    serde_json::to_string(&envelope).expect("park state serialization is infallible")
}

/// Parses, migrates and validates a save document into a park ready to replace the active one.
pub(crate) fn decode(document: &str) -> Result<GameState, SaveError> {
    if document.len() > MAX_SAVE_BYTES {
        return Err(SaveError::new(
            "too_large",
            format!("save exceeds {MAX_SAVE_BYTES} bytes"),
        ));
    }
    let value: Value = serde_json::from_str(document)
        .map_err(|error| SaveError::new("malformed", format!("save is not valid JSON: {error}")))?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(SaveError::new(
            "unsupported_format",
            format!("not a {FORMAT} document"),
        ));
    }
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| SaveError::new("invalid_save", "schema_version: missing or not a number"))?;
    if version > u64::from(SCHEMA_VERSION) {
        return Err(SaveError::new(
            "unsupported_version",
            format!("save schema {version} is newer than supported schema {SCHEMA_VERSION}"),
        ));
    }
    if version == 0 {
        return Err(SaveError::new(
            "unsupported_version",
            "save schema 0 does not exist",
        ));
    }
    let mut value = migrate(value, version as u32)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| SaveError::new("invalid_save", "save is not an object"))?;
    if let Some(key) = object.keys().find(|key| {
        !matches!(
            key.as_str(),
            "format" | "schema_version" | "content" | "park"
        )
    }) {
        return Err(SaveError::new(
            "invalid_save",
            format!("unknown field `{key}`"),
        ));
    }
    let content = object
        .remove("content")
        .ok_or_else(|| SaveError::new("invalid_save", "content: missing"))?;
    let park = object
        .remove("park")
        .ok_or_else(|| SaveError::new("invalid_save", "park: missing"))?;
    let catalog = Catalog::parse(&content.to_string())
        .map_err(|message| SaveError::new("invalid_content", message))?;
    let mut state: GameState = serde_path_to_error::deserialize(park).map_err(|error| {
        SaveError::new(
            "invalid_save",
            format!("park/{}: {}", error.path(), error.inner()),
        )
    })?;
    state.catalog = Arc::new(catalog);
    if let Some(violation) = state.save_violation() {
        return Err(SaveError::new("invalid_park", violation));
    }
    Ok(state)
}

/// Rewrites a document of schema `from` (1..=[`SCHEMA_VERSION`]) into the current schema,
/// running every later migration in order.
fn migrate(mut value: Value, from: u32) -> Result<Value, SaveError> {
    for migration in &MIGRATIONS[from as usize - 1..] {
        value = migration(value)?;
    }
    value["schema_version"] = json!(SCHEMA_VERSION);
    Ok(value)
}

/// Schema 10 (#98) gives every path tile an access class. Earlier parks had only one kind of
/// path, which guests and staff both walked, so every `"path"` tile becomes a public path.
fn v9_classify_paths(mut value: Value) -> Result<Value, SaveError> {
    let tiles = value
        .get_mut("park")
        .and_then(|park| park.get_mut("tiles"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| SaveError::new("invalid_save", "park/tiles: missing or not an array"))?;
    for tile in tiles.iter_mut().filter(|tile| *tile == "path") {
        *tile = json!({ "path": "public" });
    }
    Ok(value)
}

/// Schema 8 drops the embedded catalog's own `schema_version`: Serde's strict typed catalog is
/// the content schema and the session's `content_digest` its identity, so no hand-bumped
/// content number remains.
fn drop_content_schema_version(mut value: Value) -> Result<Value, SaveError> {
    value
        .get_mut("content")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| SaveError::new("invalid_save", "content: missing or not an object"))?
        .remove("schema_version");
    Ok(value)
}

/// Schema 2 (#91) records the park's scenario source, landmarks, unlocks and objectives.
/// Every schema 1 park was the fixed 20x14 empty starter layout with all content unlocked.
fn v1_to_v2(mut value: Value) -> Result<Value, SaveError> {
    let park = value
        .get_mut("park")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| SaveError::new("invalid_save", "park: missing or not an object"))?;
    park.insert("entrance".to_owned(), json!({ "x": 0, "y": 7 }));
    park.insert("depot".to_owned(), json!({ "x": 2, "y": 6 }));
    park.insert(
        "scenario".to_owned(),
        json!({
            "id": "empty-starter",
            "title": "Empty meadow",
            "seed": 0,
            "digest": null,
            "opening_focus": { "x": 4, "y": 7 },
        }),
    );
    park.insert("unlocks".to_owned(), json!({}));
    park.insert("objectives".to_owned(), json!([]));
    Ok(value)
}

/// Schema 3 (#93) replaces the fixed keeper-training steps with the guided opening scenario:
/// the guide's progress is the park's objective progress. Keeper-training parks (the
/// `empty-starter` scenario, now the guided `first-zoo`) receive the opening objectives,
/// unfinished, and keep their guide status; every other park's guide is off.
fn v2_to_v3(mut value: Value) -> Result<Value, SaveError> {
    let park = value
        .get_mut("park")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| SaveError::new("invalid_save", "park: missing or not an object"))?;
    let keeper_training = park
        .get("scenario")
        .and_then(|scenario| scenario.get("id"))
        .and_then(Value::as_str)
        == Some("empty-starter");
    let status = if keeper_training {
        park.get("tutorial")
            .and_then(|tutorial| tutorial.get("status"))
            .and_then(Value::as_str)
            .filter(|status| matches!(*status, "active" | "skipped" | "completed"))
            .ok_or_else(|| SaveError::new("invalid_save", "park/tutorial/status: missing"))?
            .to_owned()
    } else {
        "off".to_owned()
    };
    if keeper_training {
        let opening = scenario::Scenario::built_in(scenario::DEFAULT_SCENARIO_ID)
            .expect("the opening scenario is built in");
        park["scenario"]["id"] = json!(scenario::DEFAULT_SCENARIO_ID);
        park.insert(
            "objectives".to_owned(),
            serde_json::to_value(opening.objective_progress())
                .expect("objective serialization is infallible"),
        );
    }
    park.insert("tutorial".to_owned(), json!({ "status": status }));
    Ok(value)
}

/// Schema 4 (#94) saves the park's alerts. An older park starts without any; loading re-reads
/// the park's conditions at once, so its current problems reappear first seen at load time.
fn v3_to_v4(mut value: Value) -> Result<Value, SaveError> {
    let park = value
        .get_mut("park")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| SaveError::new("invalid_save", "park: missing or not an object"))?;
    park.insert("alerts".to_owned(), json!([]));
    Ok(value)
}

/// Schema 5 (#95) stores each habitat as its enclosed tiles instead of a rectangle, so fences
/// can take any simple orthogonal shape. Every schema 4 habitat becomes the row-major tiles of
/// its rectangle; area, capacity, fence, gate and ownership stay exactly as they were.
fn v4_to_v5(mut value: Value) -> Result<Value, SaveError> {
    let invalid = |message: &str| SaveError::new("invalid_save", message.to_owned());
    let habitats = value
        .get_mut("park")
        .and_then(|park| park.get_mut("habitats"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("park/habitats: missing or not an array"))?;
    for habitat in habitats {
        let habitat = habitat
            .as_object_mut()
            .ok_or_else(|| invalid("park/habitats: entry is not an object"))?;
        let mut field = |name: &str| {
            habitat
                .remove(name)
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| invalid(&format!("park/habitats/{name}: missing")))
        };
        let (x, y, width, height) = (field("x")?, field("y")?, field("width")?, field("height")?);
        if width == 0 || height == 0 || width > 1024 || height > 1024 {
            return Err(invalid("park/habitats: footprint out of range"));
        }
        habitat.remove("orientation");
        let tiles = x
            .checked_add(width)
            .zip(y.checked_add(height))
            .map(|_| shape::rect_tiles(x, y, width, height))
            .ok_or_else(|| invalid("park/habitats: footprint out of range"))?;
        habitat.insert(
            "tiles".to_owned(),
            serde_json::to_value(tiles).expect("tile serialization is infallible"),
        );
    }
    Ok(value)
}

/// Schema 6 (#95) moves fences into content (catalog schema 3): the one implicit fence becomes
/// a `wooden_rail` type with exactly the old segment price and upkeep, the built-in `steel_mesh`
/// type joins it, every species gains its built-in minimum fence height (0 for species the
/// built-in catalog does not know), and every habitat is fenced with the wooden rail. Prices,
/// upkeep and every habitat's outcomes stay as they were.
fn v5_to_v6(mut value: Value) -> Result<Value, SaveError> {
    let invalid = |message: &str| SaveError::new("invalid_save", message.to_owned());
    let built_in = Catalog::parse(content::DEFAULT_DOCUMENT).expect("valid built-in catalog");
    let content = value
        .get_mut("content")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("content: missing or not an object"))?;
    let mut take = |section: &str, field: &str| {
        content
            .get_mut("balance")
            .and_then(|balance| balance.get_mut(section))
            .and_then(Value::as_object_mut)
            .and_then(|section| section.remove(field))
            .ok_or_else(|| invalid(&format!("content/balance/{section}/{field}: missing")))
    };
    let segment_cents = take("construction", "fence_segment_cents")?;
    let segment_hourly_cents = take("upkeep", "fence_segment_hourly_cents")?;
    let wooden = json!({
        "key": "wooden_rail",
        "label": "Wooden rail",
        "material": "wood",
        "height_cm": built_in.fences[0].height_cm,
        "segment_cents": segment_cents,
        "segment_hourly_cents": segment_hourly_cents,
    });
    let steel = built_in
        .fences
        .iter()
        .find(|fence| fence.key == "steel_mesh")
        .expect("the built-in catalog has steel mesh");
    let minimum = |key: Option<&str>| {
        built_in
            .species
            .iter()
            .find(|species| Some(species.key.as_str()) == key)
            .map_or(0, |species| species.minimum_fence_height_cm)
    };
    let add_minimum = |species: &mut Value| -> Result<(), SaveError> {
        let height = minimum(species.get("key").and_then(Value::as_str));
        species
            .as_object_mut()
            .ok_or_else(|| invalid("species: entry is not an object"))?
            .insert("minimum_fence_height_cm".to_owned(), json!(height));
        Ok(())
    };
    for species in content
        .get_mut("species")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("content/species: missing or not an array"))?
    {
        add_minimum(species)?;
    }
    content.insert(
        "fences".to_owned(),
        json!([
            wooden,
            serde_json::to_value(steel).expect("fence serialization")
        ]),
    );
    content.insert("schema_version".to_owned(), json!(3));
    let habitats = value
        .get_mut("park")
        .and_then(|park| park.get_mut("habitats"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("park/habitats: missing or not an array"))?;
    for habitat in habitats {
        if let Some(species) = habitat.get_mut("species").filter(|s| s.is_object()) {
            add_minimum(species)?;
        }
        habitat
            .as_object_mut()
            .ok_or_else(|| invalid("park/habitats: entry is not an object"))?
            .insert("fence".to_owned(), wooden.clone());
    }
    Ok(value)
}

/// Schema 7 (#96) replaces each habitat's animal count with individual animal records. Every
/// counted animal becomes an adult with a park-wide ID allocated in habitat then slot order,
/// standing on the tile the old slot formula placed it at the saved minute, keeping its old
/// animation phase as its cosmetic seed, rested and about to choose where to walk. Animals
/// whose arrival predates the records count as arriving on day 1.
fn v6_to_v7(mut value: Value) -> Result<Value, SaveError> {
    let invalid = |message: &str| SaveError::new("invalid_save", message.to_owned());
    let park = value
        .get_mut("park")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("park: missing or not an object"))?;
    let number = |park: &serde_json::Map<String, Value>, name: &str| {
        park.get(name)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| invalid(&format!("park/{name}: missing")))
    };
    let minute_of_day = number(park, "minute_of_day")?;
    let day = number(park, "day")?;
    let now =
        u64::from(day.saturating_sub(1)) * u64::from(MINUTES_PER_DAY) + u64::from(minute_of_day);
    let mut next_id: u32 = 1;
    let habitats = park
        .get_mut("habitats")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("park/habitats: missing or not an array"))?;
    for habitat in habitats {
        let habitat = habitat
            .as_object_mut()
            .ok_or_else(|| invalid("park/habitats: entry is not an object"))?;
        let count = habitat
            .remove("animals")
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .filter(|count| *count <= 1024)
            .ok_or_else(|| invalid("park/habitats/animals: missing or out of range"))?;
        let id = number(habitat, "id")?;
        let tiles: Vec<Position> = habitat
            .get("tiles")
            .cloned()
            .and_then(|tiles| serde_json::from_value(tiles).ok())
            .ok_or_else(|| invalid("park/habitats/tiles: missing"))?;
        let species = habitat
            .get("species")
            .and_then(|species| species.get("key"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut animals = Vec::new();
        if count > 0 && (species.is_none() || tiles.is_empty()) {
            return Err(invalid(
                "park/habitats: animals without a species or enclosure",
            ));
        }
        if let Some(species) = species {
            for slot in 0..count {
                let seed = (slot.saturating_mul(17).saturating_add(id.saturating_mul(7))) % 100;
                animals.push(animal::Animal {
                    id: next_id,
                    species: species.clone(),
                    name: animal::default_name(animal::seed_for(next_id)),
                    sex: animal::sex_for(animal::seed_for(next_id)),
                    life_stage: animal::LifeStage::Adult,
                    arrived_day: 1,
                    cosmetic_seed: seed,
                    position: animal::arrival_tile(
                        &animal::roam_tiles(&tiles),
                        id,
                        slot,
                        minute_of_day,
                    ),
                    activity: animal::Activity::Resting { until_minute: now },
                    energy: 100,
                    decisions: 0,
                });
                next_id = next_id.saturating_add(1);
            }
        }
        habitat.insert(
            "animals".to_owned(),
            serde_json::to_value(animals).expect("animal serialization is infallible"),
        );
    }
    park.insert("next_animal_id".to_owned(), json!(next_id));
    Ok(value)
}

/// Schema 9 (#97) gives habitats an environment and its catalog the matching content. The flat shelter price
/// becomes the `basic_shelter` object's price beside the built-in furnishings and terrain
/// prices; every species gains its built-in habitat requirements (species the built-in catalog
/// does not know need only shelter, as before); a habitat that had a shelter gets one basic
/// shelter object on its default tile, with object IDs allocated in habitat order. Terrain
/// starts as level grass everywhere.
fn v8_add_habitat_environments(mut value: Value) -> Result<Value, SaveError> {
    let invalid = |message: &str| SaveError::new("invalid_save", message.to_owned());
    let built_in = Catalog::parse(content::DEFAULT_DOCUMENT).expect("valid built-in catalog");
    let content = value
        .get_mut("content")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("content: missing or not an object"))?;
    let balance = content
        .get_mut("balance")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("content/balance: missing"))?;
    let shelter_cents = balance
        .get_mut("habitat_care")
        .and_then(Value::as_object_mut)
        .and_then(|care| care.remove("shelter_cents"))
        .ok_or_else(|| invalid("content/balance/habitat_care/shelter_cents: missing"))?;
    balance.insert(
        "habitat_environment".to_owned(),
        serde_json::to_value(&built_in.balance.habitat_environment)
            .expect("environment cost serialization"),
    );
    let mut objects =
        serde_json::to_value(&built_in.habitat_objects).expect("habitat object serialization");
    for object in objects
        .as_array_mut()
        .expect("objects serialize as an array")
    {
        if object["key"] == "basic_shelter" {
            object["build_cents"] = shelter_cents.clone();
        }
    }
    content.insert("habitat_objects".to_owned(), objects.clone());
    let requirements = |key: Option<&str>| {
        built_in
            .species
            .iter()
            .find(|species| Some(species.key.as_str()) == key)
            .map_or_else(
                || {
                    json!({
                        "ground_cover": "grass",
                        "ground_cover_percent": 0,
                        "shallow_water_tiles": 0,
                        "deep_water_tiles": 0,
                        "shelter": true,
                        "enrichment": false,
                    })
                },
                |species| {
                    serde_json::to_value(&species.habitat).expect("requirement serialization")
                },
            )
    };
    let add_requirements = |species: &mut Value| -> Result<(), SaveError> {
        let needs = requirements(species.get("key").and_then(Value::as_str));
        species
            .as_object_mut()
            .ok_or_else(|| invalid("species: entry is not an object"))?
            .insert("habitat".to_owned(), needs);
        Ok(())
    };
    for species in content
        .get_mut("species")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("content/species: missing or not an array"))?
    {
        add_requirements(species)?;
    }
    let shelter = objects
        .as_array()
        .and_then(|objects| {
            objects
                .iter()
                .find(|object| object["key"] == "basic_shelter")
        })
        .cloned()
        .expect("the built-in catalog has a basic shelter");
    let park = value
        .get_mut("park")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("park: missing or not an object"))?;
    let mut next_id: u32 = 1;
    for habitat in park
        .get_mut("habitats")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("park/habitats: missing or not an array"))?
    {
        if let Some(species) = habitat.get_mut("species").filter(|s| s.is_object()) {
            add_requirements(species)?;
        }
        let habitat = habitat
            .as_object_mut()
            .ok_or_else(|| invalid("park/habitats: entry is not an object"))?;
        let sheltered = habitat
            .remove("has_shelter")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| invalid("park/habitats/has_shelter: missing"))?;
        let mut objects = Vec::new();
        if sheltered {
            let tiles: Vec<Position> = habitat
                .get("tiles")
                .cloned()
                .and_then(|tiles| serde_json::from_value(tiles).ok())
                .ok_or_else(|| invalid("park/habitats/tiles: missing"))?;
            let gate: Position = habitat
                .get("service_gate")
                .and_then(|gate| {
                    Some(Position {
                        x: u32::try_from(gate.get("x")?.as_u64()?).ok()?,
                        y: u32::try_from(gate.get("y")?.as_u64()?).ok()?,
                    })
                })
                .ok_or_else(|| invalid("park/habitats/service_gate: missing"))?;
            let position = environment::default_object_tile(
                &tiles,
                &environment::Environment::default(),
                gate,
            )
            .ok_or_else(|| invalid("park/habitats: no tile for the migrated shelter"))?;
            objects.push(json!({ "id": next_id, "definition": shelter, "position": position }));
            next_id = next_id.saturating_add(1);
        }
        habitat.insert(
            "environment".to_owned(),
            json!({ "terrain": [], "objects": objects }),
        );
    }
    park.insert("next_habitat_object_id".to_owned(), json!(next_id));
    Ok(value)
}

impl GameState {
    /// Structural checks a loaded park must pass beyond the per-tick invariants: dimensions,
    /// fixed landmarks, unique ordered IDs, resolvable references and value ranges.
    pub(crate) fn save_violation(&self) -> Option<String> {
        self.save_violation_result().err()
    }

    fn save_violation_result(&self) -> Result<(), String> {
        self.scenario_violation()?;
        if let Some(violation) = self.environment_save_violation() {
            return Err(violation);
        }
        if let Some(violation) = self.invariant_violation() {
            return Err(violation);
        }
        if self.day == 0 || self.minute_of_day >= MINUTES_PER_DAY {
            return Err(format!(
                "clock day {} minute {} is out of range",
                self.day, self.minute_of_day
            ));
        }
        ordered(
            "litter",
            self.litter.iter().map(|task| task.id),
            self.next_litter_id,
        )?;
        ordered(
            "maintenance",
            self.maintenance.iter().map(|task| task.id),
            self.next_maintenance_id,
        )?;
        let in_bounds = |name: &str, id: u32, x: u32, y: u32| {
            if x < self.width && y < self.height {
                Ok(())
            } else {
                Err(format!("{name} {id} stands outside the park at {x},{y}"))
            }
        };
        let percent = |name: &str, id: u32, values: &[u32]| {
            if values.iter().all(|value| *value <= 100) {
                Ok(())
            } else {
                Err(format!("{name} {id} has a rating above 100"))
            }
        };

        // Landmarks and tile ownership.
        if self.tile(self.entrance.x, self.entrance.y) != Some(TileKind::Entrance) {
            return Err("the entrance tile is missing".to_owned());
        }
        let depot = self.depot_position();
        if self.tile(depot.x, depot.y) != Some(TileKind::AnimalCareDepot) {
            return Err("the animal-care depot tile is missing".to_owned());
        }
        let habitat_ids: HashSet<u32> = self.habitats.iter().map(|h| h.id).collect();
        let concession_ids: HashSet<u32> = self.concessions.iter().map(|c| c.id).collect();
        for y in 0..self.height {
            for x in 0..self.width {
                match self.tile(x, y) {
                    Some(TileKind::Entrance) if (x, y) != (self.entrance.x, self.entrance.y) => {
                        return Err(format!("a second entrance tile at {x},{y}"));
                    }
                    Some(TileKind::AnimalCareDepot) if (x, y) != (depot.x, depot.y) => {
                        return Err(format!("a second depot tile at {x},{y}"));
                    }
                    Some(TileKind::Habitat(id)) if !habitat_ids.contains(&id) => {
                        return Err(format!("tile {x},{y} belongs to missing habitat {id}"));
                    }
                    Some(TileKind::Concession(id)) if !concession_ids.contains(&id) => {
                        return Err(format!("tile {x},{y} belongs to missing concession {id}"));
                    }
                    _ => {}
                }
            }
        }
        for habitat in &self.habitats {
            let id = habitat.id;
            if habitat
                .tiles
                .iter()
                .any(|tile| tile.x >= self.width || tile.y >= self.height)
            {
                return Err(format!("habitat {id} footprint leaves the park"));
            }
            let ordered = habitat
                .tiles
                .windows(2)
                .all(|pair| (pair[0].y, pair[0].x) < (pair[1].y, pair[1].x));
            if !ordered || shape::clockwise_fence(&habitat.tiles).is_none() {
                return Err(format!("habitat {id} is not one simple enclosure"));
            }
            if !habitat.fence_segments().contains(&habitat.service_gate) {
                return Err(format!("habitat {id} service gate is not on its fence"));
            }
            let owned = habitat
                .tiles
                .iter()
                .all(|tile| self.tile(tile.x, tile.y) == Some(TileKind::Habitat(id)));
            let tiles = self
                .tiles
                .iter()
                .filter(|tile| **tile == TileKind::Habitat(id))
                .count();
            if !owned || tiles != habitat.area() as usize {
                return Err(format!("habitat {id} tiles do not match its footprint"));
            }
            if !self.catalog.fences.contains(&habitat.fence) {
                return Err(format!(
                    "habitat {id} fence {} does not match the content",
                    habitat.fence.key
                ));
            }
            match &habitat.species {
                Some(species) if !self.catalog.species.contains(species) => {
                    return Err(format!(
                        "habitat {id} houses species {} that does not match the content",
                        species.key
                    ));
                }
                None if habitat.stocked() => {
                    return Err(format!("habitat {id} has animals but no species"));
                }
                _ => {}
            }
            percent(
                "habitat",
                id,
                &[
                    habitat.welfare,
                    habitat.food,
                    habitat.water,
                    habitat.cleanliness,
                ],
            )?;
        }
        for stand in &self.concessions {
            in_bounds("concession", stand.id, stand.x, stand.y)?;
            if self.tile(stand.x, stand.y) != Some(TileKind::Concession(stand.id)) {
                return Err(format!("concession {} has no tile", stand.id));
            }
            if !self
                .catalog
                .facilities
                .iter()
                .any(|facility| same_facility(facility, &stand.definition))
            {
                return Err(format!(
                    "concession {} uses facility {} that does not match the content",
                    stand.id, stand.definition.key
                ));
            }
            percent("concession", stand.id, &[stand.condition])?;
        }

        // Staff, tasks and their references.
        let zone_ids: HashSet<u32> = self.work_zones.iter().map(|z| z.id).collect();
        let keeper_ids: HashSet<u32> = self.keepers.iter().map(|k| k.id).collect();
        let job_ids: HashSet<u32> = self.care_jobs.iter().map(|j| j.id).collect();
        for keeper in &self.keepers {
            in_bounds("keeper", keeper.id, keeper.x, keeper.y)?;
            if keeper.zone_id.is_some_and(|zone| !zone_ids.contains(&zone)) {
                return Err(format!("keeper {} works in a missing zone", keeper.id));
            }
            if keeper
                .activity
                .job_id()
                .is_some_and(|job| !job_ids.contains(&job))
            {
                return Err(format!("keeper {} works a missing care job", keeper.id));
            }
            if keeper.schedule.categories.is_empty() {
                return Err(format!("keeper {} accepts no care categories", keeper.id));
            }
        }
        for job in &self.care_jobs {
            if !habitat_ids.contains(&job.habitat_id) {
                return Err(format!("care job {} belongs to a missing habitat", job.id));
            }
            if job.keeper_ids.iter().any(|id| !keeper_ids.contains(id)) {
                return Err(format!("care job {} reserves a missing keeper", job.id));
            }
            if job.state == CareJobState::InProgress
                && job.keeper_ids.len() != job.required_keepers as usize
            {
                return Err(format!("care job {} has an incomplete crew", job.id));
            }
        }
        let litter_ids: HashSet<u32> = self.litter.iter().map(|t| t.id).collect();
        let janitor_ids: HashSet<u32> = self.janitors.iter().map(|j| j.id).collect();
        for janitor in &self.janitors {
            in_bounds("janitor", janitor.id, janitor.x, janitor.y)?;
            if janitor
                .target_litter_id
                .is_some_and(|id| !litter_ids.contains(&id))
            {
                return Err(format!("janitor {} targets missing litter", janitor.id));
            }
        }
        for task in &self.litter {
            in_bounds("litter", task.id, task.x, task.y)?;
            if task
                .assigned_janitor_id
                .is_some_and(|id| !janitor_ids.contains(&id))
            {
                return Err(format!("litter {} is assigned a missing janitor", task.id));
            }
        }
        let maintenance_ids: HashSet<u32> = self.maintenance.iter().map(|t| t.id).collect();
        let mechanic_ids: HashSet<u32> = self.mechanics.iter().map(|m| m.id).collect();
        for mechanic in &self.mechanics {
            in_bounds("mechanic", mechanic.id, mechanic.x, mechanic.y)?;
            if mechanic
                .target_maintenance_id
                .is_some_and(|id| !maintenance_ids.contains(&id))
            {
                return Err(format!(
                    "mechanic {} targets a missing maintenance task",
                    mechanic.id
                ));
            }
        }
        for task in &self.maintenance {
            if !concession_ids.contains(&task.concession_id) {
                return Err(format!(
                    "maintenance {} belongs to a missing concession",
                    task.id
                ));
            }
            if task
                .assigned_mechanic_id
                .is_some_and(|id| !mechanic_ids.contains(&id))
            {
                return Err(format!(
                    "maintenance {} is assigned a missing mechanic",
                    task.id
                ));
            }
        }

        // Guests.
        for guest in &self.guests {
            let id = guest.id;
            in_bounds("guest", id, guest.x, guest.y)?;
            if !habitat_ids.contains(&guest.target_habitat) {
                return Err(format!("guest {id} heads to a missing habitat"));
            }
            if guest.route_index > guest.route.len() {
                return Err(format!("guest {id} is past the end of its route"));
            }
            if let Some(Position { x, y }) = guest
                .route
                .iter()
                .find(|step| step.x >= self.width || step.y >= self.height)
            {
                return Err(format!("guest {id} routes outside the park at {x},{y}"));
            }
            // Guests never use staff-only paths (#98): not where they stand, not ahead of them.
            let staff_only =
                |x: u32, y: u32| self.tile(x, y) == Some(TileKind::Path(PathAccess::Staff));
            if staff_only(guest.x, guest.y) {
                return Err(format!("guest {id} stands on a staff-only path"));
            }
            if guest.state != GuestState::Viewing
                && guest
                    .route
                    .get(guest.route_index..)
                    .unwrap_or_default()
                    .iter()
                    .any(|step| staff_only(step.x, step.y))
            {
                return Err(format!("guest {id} routes over a staff-only path"));
            }
            if guest.visited_habitats.len() > MAX_GUEST_HABITATS_PER_VISIT {
                return Err(format!("guest {id} visited too many habitats"));
            }
            percent(
                "guest",
                id,
                &[
                    guest.happiness,
                    guest.energy,
                    guest.hunger,
                    guest.thirst,
                    guest.value_perception,
                ],
            )?;
        }

        // Ledgers.
        let ledgers = std::iter::once(&self.finance_today).chain(self.finance_previous.as_ref());
        for ledger in ledgers {
            let fields = serde_json::to_value(ledger).expect("ledger serialization is infallible");
            let out_of_range = fields.as_object().into_iter().flatten().any(|(_, cents)| {
                cents
                    .as_i64()
                    .is_none_or(|cents| !(0..=content::MAX_CENTS).contains(&cents))
            });
            if out_of_range {
                return Err("a finance ledger entry is outside its range".to_owned());
            }
        }
        Ok(())
    }
}

fn ordered(name: &str, ids: impl Iterator<Item = u32>, next: u32) -> Result<(), String> {
    let mut previous = 0;
    for id in ids {
        if id <= previous || id >= next {
            return Err(format!(
                "{name} id {id} is duplicated, out of order or not yet allocated (next {next})"
            ));
        }
        previous = id;
    }
    Ok(())
}

fn same_facility(left: &content::Facility, right: &content::Facility) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}
