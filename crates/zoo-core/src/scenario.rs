//! Scenario definitions and reproducible park creation (#91).
//!
//! A scenario is a versioned, data-only template for a new park:
//!
//! ```json
//! { "format": "zoo-scenario", "schema_version": 1, "id": "first-zoo", "seed": 1,
//!   "map": { "width": 20, "height": 14, "terrain": [] }, "entrance": {..}, "depot": {..},
//!   "starting_cash_cents": 5000000, "unlocks": {..}, "setup": [..], "tutorial": "none",
//!   "opening_focus": {..}, "objectives": [..] }
//! ```
//!
//! Creation is deterministic: the same scenario and content always produce the same park, and
//! the same park plus the same commands always produce the same results. Starting objects are
//! built by running each `setup` step through the live command rules (placement, occupancy,
//! reachability, content and reference checks), so an authored park can only contain what a
//! player could build. They are free initial state: cash and the ledger are set after setup,
//! never charged for it. The finished park must then pass the same structural validation as a
//! loaded save, or creation fails before anything replaces the active park.
//!
//! A park keeps an immutable [`ScenarioIdentity`] (id, seed, digest) and its own copy of the
//! unlocks and objectives, so later edits to a scenario never mutate parks created from it.
//! The simulation currently draws no random numbers; `seed` is recorded so future random
//! streams derive from it rather than from wall-clock entropy.
use super::{
    ActionResult, GameState, Position, TileKind,
    care::CareJobState,
    content::{self, Catalog},
    session::{DomainEvent, fnv1a64},
    tutorial::Tutorial,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) const DEFAULT_SCENARIO_ID: &str = "first-zoo";
pub(crate) const MIN_MAP_DIMENSION: u32 = 8;
pub(crate) const MAX_MAP_DIMENSION: u32 = 64;
pub(crate) const MAX_MAP_TILES: u32 = MAX_MAP_DIMENSION * MAX_MAP_DIMENSION;
const FORMAT: &str = "zoo-scenario";
const SCHEMA_VERSION: u32 = 1;
const MAX_DOCUMENT_BYTES: usize = 128 * 1024;
const MAX_SETUP_STEPS: usize = 512;
const MAX_TERRAIN_PATCHES: usize = 64;
const MAX_OBJECTIVES: usize = 16;
const MAX_ADOPT_COUNT: u32 = 12;
const MAX_ID_BYTES: usize = 64;
const MAX_TEXT_BYTES: usize = 400;
/// A stand is serviced once a mechanic has nothing urgent to do there.
const SERVICED_CONDITION: u32 = super::MAINTENANCE_SERVICE_THRESHOLD;

/// Scenarios shipped with the game, in picker order.
const BUILT_IN: [&str; 4] = [
    include_str!("../scenarios/first-zoo.json"),
    include_str!("../scenarios/partly-prepared.json"),
    include_str!("../scenarios/living-starter.json"),
    include_str!("../scenarios/recovery.json"),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scenario {
    format: String,
    schema_version: u32,
    pub id: String,
    title: String,
    summary: String,
    seed: u32,
    map: MapDefinition,
    entrance: Position,
    depot: Position,
    starting_cash_cents: i64,
    #[serde(default)]
    unlocks: Unlocks,
    #[serde(default)]
    setup: Vec<SetupStep>,
    tutorial: TutorialMode,
    /// Where the camera opens: framing the first playable objective.
    opening_focus: Position,
    #[serde(default)]
    objectives: Vec<ObjectiveDefinition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapDefinition {
    width: u32,
    height: u32,
    #[serde(default)]
    terrain: Vec<TerrainPatch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TerrainKind {
    Water,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainPatch {
    kind: TerrainKind,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TutorialMode {
    /// The objectives are a guided introduction (#93): the next one shows its hint, the park's
    /// current blocker and a tool pointer, and the player may skip or restart the guide.
    Guided,
    /// The objectives are plain scenario goals.
    None,
}

/// What a park may build and adopt. `None` makes every content entry available.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Unlocks {
    #[serde(default)]
    species: Option<Vec<String>>,
    #[serde(default)]
    facilities: Option<Vec<String>>,
}

impl Unlocks {
    pub(crate) fn species_unlocked(&self, key: &str) -> bool {
        self.species
            .as_ref()
            .is_none_or(|keys| keys.iter().any(|unlocked| unlocked == key))
    }

    pub(crate) fn facility_unlocked(&self, key: &str) -> bool {
        self.facilities
            .as_ref()
            .is_none_or(|keys| keys.iter().any(|unlocked| unlocked == key))
    }

    fn validate(&self, catalog: &Catalog) -> Result<(), String> {
        let check = |name: &str, keys: &Option<Vec<String>>, known: &dyn Fn(&str) -> bool| {
            let Some(keys) = keys else {
                return Ok(());
            };
            let mut seen = HashSet::new();
            for (i, key) in keys.iter().enumerate() {
                if !seen.insert(key) {
                    return Err(format!("unlocks/{name}/{i}: duplicate key {key}"));
                }
                if !known(key) {
                    return Err(format!("unlocks/{name}/{i}: unknown content key {key}"));
                }
            }
            Ok(())
        };
        check("species", &self.species, &|key| {
            catalog.species.iter().any(|species| species.key == key)
        })?;
        check("facilities", &self.facilities, &|key| {
            catalog
                .facilities
                .iter()
                .any(|facility| facility.key == key)
        })
    }
}

/// One authored starting object or condition, applied through the live command rules.
/// Habitat, zone, keeper and concession IDs are the stable IDs the park assigns in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SetupStep {
    /// A path stroke from `from` to `to`, built by the same stroke rules as the Path tool.
    Path {
        from: Position,
        to: Position,
    },
    Habitat {
        from: Position,
        to: Position,
    },
    ServiceGate {
        habitat: u32,
        x: u32,
        y: u32,
        side: String,
    },
    Concession {
        x: u32,
        y: u32,
        facility: String,
    },
    WorkZone {
        from: Position,
        to: Position,
    },
    HireKeeper,
    HireJanitor,
    HireMechanic,
    AssignKeeper {
        keeper: u32,
        zone: u32,
    },
    Adopt {
        habitat: u32,
        species: String,
        count: u32,
    },
    Shelter {
        habitat: u32,
    },
    /// Sets the depot's feed stock.
    FeedCrates {
        crates: u32,
    },
    /// Overrides a habitat's ratings to author existing problems.
    HabitatCondition {
        habitat: u32,
        #[serde(default)]
        food: Option<u32>,
        #[serde(default)]
        water: Option<u32>,
        #[serde(default)]
        cleanliness: Option<u32>,
        #[serde(default)]
        welfare: Option<u32>,
    },
    /// Overrides a stand's maintenance condition to author a worn or broken stand.
    ConcessionCondition {
        concession: u32,
        condition: u32,
    },
}

impl SetupStep {
    fn kind(&self) -> &'static str {
        match self {
            Self::Path { .. } => "path",
            Self::Habitat { .. } => "habitat",
            Self::ServiceGate { .. } => "service_gate",
            Self::Concession { .. } => "concession",
            Self::WorkZone { .. } => "work_zone",
            Self::HireKeeper => "hire_keeper",
            Self::HireJanitor => "hire_janitor",
            Self::HireMechanic => "hire_mechanic",
            Self::AssignKeeper { .. } => "assign_keeper",
            Self::Adopt { .. } => "adopt",
            Self::Shelter { .. } => "shelter",
            Self::FeedCrates { .. } => "feed_crates",
            Self::HabitatCondition { .. } => "habitat_condition",
            Self::ConcessionCondition { .. } => "concession_condition",
        }
    }
}

/// A state-backed goal. Progress is read from authoritative park state after every command
/// and advance, never from UI clicks or prescribed coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Goal {
    Habitats {
        at_least: u32,
    },
    StockedHabitats {
        at_least: u32,
    },
    Animals {
        at_least: u32,
    },
    Keepers {
        at_least: u32,
    },
    Concessions {
        at_least: u32,
    },
    Guests {
        at_least: u32,
    },
    CashCents {
        at_least: i64,
    },
    FeedCrates {
        at_least: u32,
    },
    /// Every stocked habitat (and at least one) has at least this welfare.
    Welfare {
        at_least: u32,
    },
    /// Every habitat (and at least one) has a service gate keepers can reach.
    GatesReachable,
    /// Every concession (and at least one) is in serviceable condition.
    ConcessionsServiced,
    /// Habitats whose service gate lies inside a work zone.
    ZonedHabitats {
        at_least: u32,
    },
    /// Habitats whose gate's work zone has at least one keeper.
    StaffedHabitats {
        at_least: u32,
    },
    ShelteredHabitats {
        at_least: u32,
    },
    /// Stocked habitats whose group meets its species' social minimum without crowding.
    SuitableGroups {
        at_least: u32,
    },
    /// Care jobs the habitats' schedules have created, open or resolved.
    CareJobs {
        at_least: u32,
    },
    CareJobsCompleted {
        at_least: u32,
    },
    /// Care tasks the player has changed from the species defaults.
    CustomizedCareTasks {
        at_least: u32,
    },
}

impl Goal {
    /// Whether the goal describes something the park is built of, which undo or demolition
    /// can take away again; the others are moments (a job done, a crowd, a welfare level).
    pub(crate) fn structural(&self) -> bool {
        matches!(
            self,
            Goal::Habitats { .. }
                | Goal::StockedHabitats { .. }
                | Goal::Keepers { .. }
                | Goal::Concessions { .. }
                | Goal::GatesReachable
                | Goal::ZonedHabitats { .. }
                | Goal::StaffedHabitats { .. }
                | Goal::ShelteredHabitats { .. }
                | Goal::SuitableGroups { .. }
                | Goal::CustomizedCareTasks { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectiveDefinition {
    id: String,
    title: String,
    goal: Goal,
    /// The tile the camera frames for this objective.
    focus: Position,
    /// One useful next action, shown while this is the next objective.
    #[serde(default)]
    hint: Option<String>,
    /// The tool or object the hint points at.
    #[serde(default)]
    tool: Option<GuideTool>,
}

/// What an objective's hint points at; the HUD maps it to a tool or inspector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GuideTool {
    Path,
    Habitat,
    Gate,
    Zone,
    Depot,
    /// The inspector of the habitat being taught.
    HabitatInspector,
    Concession,
    /// The speed controls: the next step needs simulated time to pass.
    Speed,
}

/// A park's own copy of one objective; it latches once reached.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObjectiveProgress {
    pub id: String,
    title: String,
    pub goal: Goal,
    focus: Position,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub tool: Option<GuideTool>,
    pub done: bool,
}

/// The immutable source of a park, recorded in its saves and diagnostics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScenarioIdentity {
    pub id: String,
    pub title: String,
    pub seed: u32,
    /// Digest of the scenario document; `None` for parks from saves that predate scenarios.
    pub digest: Option<String>,
    pub opening_focus: Position,
}

#[derive(Serialize)]
pub(crate) struct ScenarioSummary<'a> {
    id: &'a str,
    title: &'a str,
    summary: &'a str,
    width: u32,
    height: u32,
    starting_cash_cents: i64,
    /// The objectives are a guided introduction with hints.
    guided: bool,
    objectives: Vec<&'a str>,
}

#[derive(Serialize)]
pub(crate) struct ObjectiveView {
    id: String,
    title: String,
    done: bool,
    focus: Position,
    hint: Option<String>,
    tool: Option<GuideTool>,
}

#[derive(Serialize)]
pub(crate) struct ScenarioView {
    id: String,
    title: String,
    seed: u32,
    /// Where the camera opens: the scenario-authored frame of its first objective.
    opening_focus: Position,
    objectives: Vec<ObjectiveView>,
}

fn text(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}: expected 1..={max} bytes of text"));
    }
    Ok(())
}

fn slug(value: &str, field: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= MAX_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{field}: expected 1..={MAX_ID_BYTES} lowercase letters, digits or hyphens"
        ))
    }
}

impl Scenario {
    /// Parses and structurally validates a scenario document. Content references and
    /// placement are checked when the park is created against a catalog.
    pub(crate) fn parse(document: &str) -> Result<Self, String> {
        if document.len() > MAX_DOCUMENT_BYTES {
            return Err(format!("scenario exceeds {MAX_DOCUMENT_BYTES} bytes"));
        }
        let mut deserializer = serde_json::Deserializer::from_str(document);
        let scenario: Self = serde_path_to_error::deserialize(&mut deserializer)
            .map_err(|error| format!("{}: {}", error.path(), error.inner()))?;
        deserializer
            .end()
            .map_err(|error| format!("trailing data: {error}"))?;
        scenario.validate()?;
        Ok(scenario)
    }

    fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("format: expected {FORMAT}"));
        }
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "schema_version: {} is not supported (expected {SCHEMA_VERSION})",
                self.schema_version
            ));
        }
        slug(&self.id, "id")?;
        text(&self.title, "title", MAX_TEXT_BYTES)?;
        text(&self.summary, "summary", MAX_TEXT_BYTES)?;
        let (width, height) = (self.map.width, self.map.height);
        let dimensions = MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION;
        if !dimensions.contains(&width) || !dimensions.contains(&height) {
            return Err(format!(
                "map: {width}x{height} is outside {MIN_MAP_DIMENSION}..={MAX_MAP_DIMENSION} tiles per side"
            ));
        }
        let inside = |field: &str, at: Position| {
            if at.x < width && at.y < height {
                Ok(())
            } else {
                Err(format!("{field}: {},{} is outside the map", at.x, at.y))
            }
        };
        inside("entrance", self.entrance)?;
        let edge = self.entrance.x == 0
            || self.entrance.y == 0
            || self.entrance.x == width - 1
            || self.entrance.y == height - 1;
        if !edge {
            return Err("entrance: must lie on the map edge".to_owned());
        }
        inside("depot", self.depot)?;
        if self.depot == self.entrance {
            return Err("depot: overlaps the entrance".to_owned());
        }
        inside("opening_focus", self.opening_focus)?;
        if !(0..=content::MAX_CENTS).contains(&self.starting_cash_cents) {
            return Err(format!(
                "starting_cash_cents: expected 0..={}",
                content::MAX_CENTS
            ));
        }
        if self.map.terrain.len() > MAX_TERRAIN_PATCHES {
            return Err(format!(
                "map/terrain: at most {MAX_TERRAIN_PATCHES} patches"
            ));
        }
        for (i, patch) in self.map.terrain.iter().enumerate() {
            let fits = patch.width > 0
                && patch.height > 0
                && patch.x.checked_add(patch.width).is_some_and(|r| r <= width)
                && patch
                    .y
                    .checked_add(patch.height)
                    .is_some_and(|b| b <= height);
            if !fits {
                return Err(format!("map/terrain/{i}: patch leaves the map"));
            }
            let covers = |at: Position| {
                (patch.x..patch.x + patch.width).contains(&at.x)
                    && (patch.y..patch.y + patch.height).contains(&at.y)
            };
            if covers(self.entrance) || covers(self.depot) {
                return Err(format!("map/terrain/{i}: covers the entrance or depot"));
            }
        }
        if self.setup.len() > MAX_SETUP_STEPS {
            return Err(format!("setup: at most {MAX_SETUP_STEPS} steps"));
        }
        if self.objectives.len() > MAX_OBJECTIVES {
            return Err(format!("objectives: at most {MAX_OBJECTIVES}"));
        }
        let mut ids = HashSet::new();
        for (i, objective) in self.objectives.iter().enumerate() {
            slug(&objective.id, &format!("objectives/{i}/id"))?;
            if !ids.insert(&objective.id) {
                return Err(format!("objectives/{i}/id: duplicate objective id"));
            }
            text(
                &objective.title,
                &format!("objectives/{i}/title"),
                MAX_TEXT_BYTES,
            )?;
            inside(&format!("objectives/{i}/focus"), objective.focus)?;
            if let Some(hint) = &objective.hint {
                text(hint, &format!("objectives/{i}/hint"), MAX_TEXT_BYTES)?;
            }
        }
        if self.tutorial == TutorialMode::Guided && self.objectives.is_empty() {
            return Err("tutorial: a guided scenario needs objectives".to_owned());
        }
        Ok(())
    }

    pub(crate) fn built_in(id: &str) -> Option<Self> {
        Self::built_ins()
            .into_iter()
            .find(|scenario| scenario.id == id)
    }

    pub(crate) fn built_ins() -> Vec<Self> {
        BUILT_IN
            .iter()
            .map(|document| Self::parse(document).expect("built-in scenarios are valid"))
            .collect()
    }

    pub(crate) fn summary(&self) -> ScenarioSummary<'_> {
        ScenarioSummary {
            id: &self.id,
            title: &self.title,
            summary: &self.summary,
            width: self.map.width,
            height: self.map.height,
            starting_cash_cents: self.starting_cash_cents,
            guided: self.tutorial == TutorialMode::Guided,
            objectives: self
                .objectives
                .iter()
                .map(|objective| objective.title.as_str())
                .collect(),
        }
    }

    /// Stable digest of the scenario's canonical serialization (whitespace-independent).
    pub(crate) fn digest(&self) -> String {
        fnv1a64(
            serde_json::to_string(self)
                .expect("scenario serialization is infallible")
                .as_bytes(),
        )
    }

    fn identity(&self) -> ScenarioIdentity {
        ScenarioIdentity {
            id: self.id.clone(),
            title: self.title.clone(),
            seed: self.seed,
            digest: Some(self.digest()),
            opening_focus: self.opening_focus,
        }
    }

    /// A park's own, unfinished copy of this scenario's objectives.
    pub(crate) fn objective_progress(&self) -> Vec<ObjectiveProgress> {
        self.objectives
            .iter()
            .map(|objective| ObjectiveProgress {
                id: objective.id.clone(),
                title: objective.title.clone(),
                goal: objective.goal.clone(),
                focus: objective.focus,
                hint: objective.hint.clone(),
                tool: objective.tool,
                done: false,
            })
            .collect()
    }

    /// Builds a new park from this scenario on `catalog`, or explains why it cannot start.
    pub(crate) fn create(&self, catalog: Arc<Catalog>) -> Result<GameState, String> {
        self.unlocks.validate(&catalog)?;
        let mut state = GameState::blank(
            catalog,
            self.map.width,
            self.map.height,
            self.entrance,
            self.depot,
            self.identity(),
        );
        for patch in &self.map.terrain {
            for y in patch.y..patch.y + patch.height {
                for x in patch.x..patch.x + patch.width {
                    state.set_tile(x, y, TileKind::Water);
                }
            }
        }
        // Authored objects are free: build them on an unlimited budget, then set the
        // scenario's cash and a clean ledger.
        state.cash_cents = content::MAX_CENTS;
        for (i, step) in self.setup.iter().enumerate() {
            let result = state.apply_setup_step(step);
            if !result.ok {
                return Err(format!("setup/{i} ({}): {}", step.kind(), result.message));
            }
        }
        state.cash_cents = self.starting_cash_cents;
        state.finance_today = super::FinanceLedger::default();
        // Authored objects are the starting park, not player construction: nothing to undo.
        state.construction.clear();
        state.unlocks = self.unlocks.clone();
        if state.depot_staff_spawn().is_none() {
            return Err(
                "depot: no path connects the depot to the entrance; staff could not reach the park"
                    .to_owned(),
            );
        }
        state.objectives = self.objective_progress();
        state.tutorial = match self.tutorial {
            TutorialMode::Guided => Tutorial::active(),
            TutorialMode::None => Tutorial::off(),
        };
        state.update_objectives();
        state.update_tutorial();
        state.pending_events.clear();
        if let Some(violation) = state.save_violation() {
            return Err(format!("the created park is invalid: {violation}"));
        }
        Ok(state)
    }
}

/// The built-in scenarios for the New park picker.
pub(crate) fn catalog_json() -> String {
    let scenarios = Scenario::built_ins();
    let summaries: Vec<ScenarioSummary<'_>> = scenarios.iter().map(Scenario::summary).collect();
    serde_json::to_string(&summaries).expect("scenario summaries serialize")
}

fn rating(value: Option<u32>) -> Result<Option<u32>, ActionResult> {
    match value {
        Some(value) if value > 100 => Err(ActionResult::error("ratings are 0..=100")),
        other => Ok(other),
    }
}

impl GameState {
    fn apply_setup_step(&mut self, step: &SetupStep) -> ActionResult {
        match step {
            SetupStep::Path { from, to } => self.build_path_stroke(&[from.x, from.y, to.x, to.y]),
            SetupStep::Habitat { from, to } => self.place_habitat_rect(from.x, from.y, to.x, to.y),
            SetupStep::ServiceGate {
                habitat,
                x,
                y,
                side,
            } => self.set_service_gate(*habitat, *x, *y, side),
            SetupStep::Concession { x, y, facility } => self.place_concession(*x, *y, facility),
            SetupStep::WorkZone { from, to } => self.create_work_zone(from.x, from.y, to.x, to.y),
            SetupStep::HireKeeper => self.hire_keeper(),
            SetupStep::HireJanitor => self.hire_janitor(),
            SetupStep::HireMechanic => self.hire_mechanic(),
            SetupStep::AssignKeeper { keeper, zone } => self.assign_keeper_zone(*keeper, *zone),
            SetupStep::Adopt {
                habitat,
                species,
                count,
            } => {
                if !(1..=MAX_ADOPT_COUNT).contains(count) {
                    return ActionResult::error(format!("count: expected 1..={MAX_ADOPT_COUNT}"));
                }
                for _ in 0..*count {
                    let result = self.adopt(*habitat, species);
                    if !result.ok {
                        return result;
                    }
                }
                ActionResult::ok("Animals adopted")
            }
            SetupStep::Shelter { habitat } => self.add_shelter(*habitat),
            SetupStep::FeedCrates { crates } => {
                self.feed_crates = *crates;
                ActionResult::ok("Feed stocked")
            }
            SetupStep::HabitatCondition {
                habitat,
                food,
                water,
                cleanliness,
                welfare,
            } => {
                let ratings = (|| {
                    Ok::<_, ActionResult>((
                        rating(*food)?,
                        rating(*water)?,
                        rating(*cleanliness)?,
                        rating(*welfare)?,
                    ))
                })();
                let (food, water, cleanliness, welfare) = match ratings {
                    Ok(ratings) => ratings,
                    Err(error) => return error,
                };
                let Some(target) = self.habitats.iter_mut().find(|h| h.id == *habitat) else {
                    return ActionResult::error(format!("habitat {habitat} does not exist"));
                };
                target.food = food.unwrap_or(target.food);
                target.water = water.unwrap_or(target.water);
                target.cleanliness = cleanliness.unwrap_or(target.cleanliness);
                target.welfare = welfare.unwrap_or(target.welfare);
                ActionResult::ok("Habitat condition set")
            }
            SetupStep::ConcessionCondition {
                concession,
                condition,
            } => {
                if *condition > 100 {
                    return ActionResult::error("ratings are 0..=100");
                }
                let Some(stand) = self.concessions.iter_mut().find(|c| c.id == *concession) else {
                    return ActionResult::error(format!("concession {concession} does not exist"));
                };
                stand.condition = *condition;
                ActionResult::ok("Concession condition set")
            }
        }
    }

    pub(crate) fn goal_reached(&self, goal: &Goal) -> bool {
        let count = |n: usize, at_least: u32| n >= at_least as usize;
        let stocked = || self.habitats.iter().filter(|habitat| habitat.stocked());
        match goal {
            Goal::Habitats { at_least } => count(self.habitats.len(), *at_least),
            Goal::StockedHabitats { at_least } => count(stocked().count(), *at_least),
            Goal::Animals { at_least } => {
                let animals: u64 = self
                    .habitats
                    .iter()
                    .map(|h| u64::from(h.animal_count()))
                    .sum();
                animals >= u64::from(*at_least)
            }
            Goal::Keepers { at_least } => count(self.keepers.len(), *at_least),
            Goal::Concessions { at_least } => count(self.concessions.len(), *at_least),
            Goal::Guests { at_least } => count(self.guests.len(), *at_least),
            Goal::CashCents { at_least } => self.cash_cents >= *at_least,
            Goal::FeedCrates { at_least } => self.feed_crates >= *at_least,
            Goal::Welfare { at_least } => {
                stocked().next().is_some() && stocked().all(|h| h.welfare >= *at_least)
            }
            Goal::GatesReachable => {
                !self.habitats.is_empty()
                    && self
                        .habitats
                        .iter()
                        .all(|habitat| self.service_gate_reachable(habitat))
            }
            Goal::ConcessionsServiced => {
                !self.concessions.is_empty()
                    && self
                        .concessions
                        .iter()
                        .all(|stand| stand.condition >= SERVICED_CONDITION)
            }
            Goal::ZonedHabitats { at_least } => count(
                self.habitats
                    .iter()
                    .filter(|habitat| self.habitat_zone(habitat).is_some())
                    .count(),
                *at_least,
            ),
            Goal::StaffedHabitats { at_least } => count(
                self.habitats
                    .iter()
                    .filter(|habitat| self.zone_staffing_problem(habitat).is_none())
                    .count(),
                *at_least,
            ),
            Goal::ShelteredHabitats { at_least } => count(
                self.habitats.iter().filter(|h| h.has_shelter).count(),
                *at_least,
            ),
            Goal::SuitableGroups { at_least } => count(
                stocked()
                    .filter(|h| h.social_score() >= 100 && h.space_score() >= 100)
                    .count(),
                *at_least,
            ),
            Goal::CareJobs { at_least } => count(self.care_jobs.len(), *at_least),
            Goal::CareJobsCompleted { at_least } => count(
                self.care_jobs
                    .iter()
                    .filter(|job| job.state == CareJobState::Completed)
                    .count(),
                *at_least,
            ),
            Goal::CustomizedCareTasks { at_least } => count(
                self.habitats
                    .iter()
                    .filter_map(|h| h.care_schedule.as_ref())
                    .flat_map(|schedule| &schedule.tasks)
                    .filter(|task| task.customized)
                    .count(),
                *at_least,
            ),
        }
    }

    /// Latches every objective the park now satisfies; called after each command and advance.
    pub(crate) fn update_objectives(&mut self) {
        let newly: Vec<usize> = self
            .objectives
            .iter()
            .enumerate()
            .filter(|(_, objective)| !objective.done && self.goal_reached(&objective.goal))
            .map(|(index, _)| index)
            .collect();
        for index in newly {
            self.objectives[index].done = true;
            self.pending_events.push(DomainEvent {
                tick: self.elapsed_minutes,
                kind: "objective_completed",
                entity_id: u32::try_from(index).ok(),
            });
        }
    }

    pub(crate) fn scenario_view(&self) -> ScenarioView {
        ScenarioView {
            id: self.scenario.id.clone(),
            title: self.scenario.title.clone(),
            seed: self.scenario.seed,
            opening_focus: self.scenario.opening_focus,
            objectives: self
                .objectives
                .iter()
                .map(|objective| ObjectiveView {
                    id: objective.id.clone(),
                    title: objective.title.clone(),
                    done: objective.done,
                    focus: objective.focus,
                    hint: objective.hint.clone(),
                    tool: objective.tool,
                })
                .collect(),
        }
    }

    /// Structural checks of the scenario-owned park fields, run with the save validation.
    pub(crate) fn scenario_violation(&self) -> Result<(), String> {
        let dimensions = MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION;
        if !dimensions.contains(&self.width) || !dimensions.contains(&self.height) {
            return Err(format!(
                "park is {}x{}, outside {MIN_MAP_DIMENSION}..={MAX_MAP_DIMENSION} tiles per side",
                self.width, self.height
            ));
        }
        if self.tiles.len() != (self.width * self.height) as usize {
            return Err("park tiles do not match its dimensions".to_owned());
        }
        self.unlocks.validate(&self.catalog)?;
        let mut ids = HashSet::new();
        for objective in &self.objectives {
            if !ids.insert(&objective.id) {
                return Err(format!("duplicate objective {}", objective.id));
            }
            if objective.focus.x >= self.width || objective.focus.y >= self.height {
                return Err(format!(
                    "objective {} focuses outside the park",
                    objective.id
                ));
            }
        }
        let focus = self.scenario.opening_focus;
        if focus.x >= self.width || focus.y >= self.height {
            return Err("the opening focus lies outside the park".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(id: &str) -> GameState {
        Scenario::built_in(id)
            .expect("built in")
            .create(super::super::default_catalog())
            .expect("valid scenario")
    }

    type Edit = dyn FnOnce(&mut serde_json::Value);

    fn with(edit: impl FnOnce(&mut serde_json::Value)) -> Result<GameState, String> {
        let mut value: serde_json::Value = serde_json::from_str(BUILT_IN[2]).unwrap();
        edit(&mut value);
        Scenario::parse(&value.to_string())?.create(super::super::default_catalog())
    }

    #[test]
    fn every_built_in_scenario_creates_a_valid_park() {
        let scenarios = Scenario::built_ins();
        let ids: HashSet<&str> = scenarios.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), BUILT_IN.len(), "scenario ids are unique");
        for scenario in &scenarios {
            let park = scenario
                .create(super::super::default_catalog())
                .unwrap_or_else(|error| panic!("{}: {error}", scenario.id));
            assert_eq!(park.scenario.id, scenario.id);
            assert_eq!(park.cash_cents, scenario.starting_cash_cents);
            assert_eq!(park.finance_today, super::super::FinanceLedger::default());
            assert!(park.pending_events.is_empty());
        }
    }

    trait TutorialStatusText {
        fn tutorial_view_status(&self) -> String;
    }

    impl TutorialStatusText for GameState {
        fn tutorial_view_status(&self) -> String {
            serde_json::to_value(self.tutorial_view()).unwrap()["status"]
                .as_str()
                .unwrap()
                .to_owned()
        }
    }

    #[test]
    fn starting_parks_differ_as_authored() {
        let empty = built("first-zoo");
        assert!(empty.habitats.is_empty());
        assert_eq!(empty.tutorial_view_status(), "active");
        let partly = built("partly-prepared");
        assert!(!partly.habitats.is_empty());
        assert!(partly.habitats.iter().all(|habitat| !habitat.stocked()));
        assert_eq!(partly.tutorial_view_status(), "off");
        let living = built("living-starter");
        assert!(living.habitats.iter().any(|habitat| habitat.stocked()));
        assert!(!living.keepers.is_empty() && !living.concessions.is_empty());
        let recovery = built("recovery");
        assert!(recovery.habitats.iter().any(|habitat| habitat.water < 40));
        assert!(
            recovery
                .concessions
                .iter()
                .any(|stand| stand.condition < SERVICED_CONDITION)
        );
        let sizes: HashSet<(u32, u32)> = [&empty, &partly, &living, &recovery]
            .iter()
            .map(|park| (park.width, park.height))
            .collect();
        assert_eq!(sizes.len(), 4, "every fixture uses its own map size");
    }

    #[test]
    fn creation_is_deterministic() {
        for scenario in Scenario::built_ins() {
            let a = super::super::save::encode(
                &scenario.create(super::super::default_catalog()).unwrap(),
            );
            let b = super::super::save::encode(
                &scenario.create(super::super::default_catalog()).unwrap(),
            );
            assert_eq!(a, b, "{}", scenario.id);
        }
    }

    #[test]
    fn invalid_scenarios_are_rejected_with_a_reason() {
        let cases: [(&str, Box<Edit>); 9] = [
            ("map: 4x20", Box::new(|v| v["map"]["width"] = 4.into())),
            ("map: 65x", Box::new(|v| v["map"]["width"] = 65.into())),
            (
                "entrance: must lie on the map edge",
                Box::new(|v| v["entrance"] = serde_json::json!({"x": 3, "y": 3})),
            ),
            (
                "patch leaves the map",
                Box::new(|v| {
                    v["map"]["terrain"] = serde_json::json!([
                        {"kind": "water", "x": 4294967295u32, "y": 0, "width": 2, "height": 1}
                    ])
                }),
            ),
            (
                "setup/",
                Box::new(|v| {
                    // The same fence shifted by one tile overlaps the first habitat.
                    let setup = v["setup"].as_array_mut().unwrap();
                    let mut overlapping = setup
                        .iter()
                        .find(|step| step["kind"] == "habitat")
                        .unwrap()
                        .clone();
                    for end in ["from", "to"] {
                        let x = overlapping[end]["x"].as_u64().unwrap();
                        overlapping[end]["x"] = (x + 1).into();
                    }
                    setup.push(overlapping);
                }),
            ),
            (
                "no path connects the depot",
                Box::new(|v| v["setup"] = serde_json::json!([])),
            ),
            (
                "unknown content key",
                Box::new(|v| v["unlocks"]["species"] = serde_json::json!(["dragon"])),
            ),
            (
                "Unknown species",
                Box::new(|v| {
                    v["setup"].as_array_mut().unwrap().push(
                        serde_json::json!({"kind": "adopt", "habitat": 1, "species": "dragon", "count": 1}),
                    )
                }),
            ),
            (
                "keeper",
                Box::new(|v| {
                    v["setup"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({"kind": "assign_keeper", "keeper": 99, "zone": 1}))
                }),
            ),
        ];
        for (expected, edit) in cases {
            let error = with(edit).expect_err(expected);
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }

    #[test]
    fn unknown_fields_and_versions_are_rejected() {
        assert!(with(|v| v["surprise"] = 1.into()).is_err());
        assert!(with(|v| v["schema_version"] = 2.into()).is_err());
        assert!(with(|v| v["setup"].as_array_mut().unwrap()[0]["extra"] = 1.into()).is_err());
    }

    #[test]
    fn locked_content_cannot_be_adopted_or_built() {
        let mut park = with(|v| {
            v["unlocks"] = serde_json::json!({"species": ["capybara"], "facilities": ["food"]})
        })
        .unwrap();
        let habitat = park.habitats[0].id;
        let result = park.adopt(habitat, "giraffe");
        assert!(
            !result.ok && result.message.contains("not available"),
            "{}",
            result.message
        );
        assert_eq!(park.species_catalog().len(), 1);
    }
}
