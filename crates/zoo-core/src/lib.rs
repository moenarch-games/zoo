mod alerts;
mod animal;
mod care;
mod construction;
mod content;
mod enclosure;
mod environment;
mod gate;
mod overlay;
mod save;
mod scenario;
mod session;
mod shape;
mod tutorial;
mod zone;

use content::{Catalog, Facility, Species};
use enclosure::PlacementEvaluation;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

// The built-in empty starter layout, which most rule tests build on.
#[cfg(test)]
const ENTRANCE_X: u32 = 0;
#[cfg(test)]
const ENTRANCE_Y: u32 = 7;
#[cfg(test)]
const PATH_COST: i64 = 1_000;
const LEGACY_HABITAT_WIDTH: u32 = 4;
const LEGACY_HABITAT_HEIGHT: u32 = 3;
#[cfg(test)]
const HABITAT_BASE_COST: i64 = 10_000;
#[cfg(test)]
const HABITAT_TILE_COST: i64 = 3_000;
#[cfg(test)]
const FENCE_SEGMENT_COST: i64 = 1_500;
const MIN_HABITAT_DIMENSION: u32 = 3;
const MAX_HABITAT_AREA: u64 = 100;
#[cfg(test)]
const ADMISSION_PRICE: i64 = 1_200;
#[cfg(test)]
const WATER_REFILL_COST: i64 = 800;
const CARE_DECAY_INTERVAL_MINUTES: u32 = 15;
#[cfg(test)]
const FOOD_STAND_BUILD_COST: i64 = 18_000;
#[cfg(test)]
const FOOD_PRICE: i64 = 500;
#[cfg(test)]
const DRINK_PRICE: i64 = 350;
const FOOD_BUY_THRESHOLD: u32 = 20;
const DRINK_BUY_THRESHOLD: u32 = 20;
#[cfg(test)]
const ANIMAL_CARE_DEPOT_X: u32 = 2;
#[cfg(test)]
const ANIMAL_CARE_DEPOT_Y: u32 = ENTRANCE_Y - 1;
#[cfg(test)]
const FEED_BATCH_COST: i64 = 6_000;
const FEED_BATCH_CRATES: u32 = 10;
#[cfg(test)]
const KEEPER_HIRE_COST: i64 = 25_000;
#[cfg(test)]
const KEEPER_HOURLY_WAGE: i64 = 1_200;
#[cfg(test)]
const JANITOR_HIRE_COST: i64 = 18_000;
#[cfg(test)]
const JANITOR_HOURLY_WAGE: i64 = 900;
#[cfg(test)]
const MECHANIC_HIRE_COST: i64 = 22_000;
#[cfg(test)]
const MECHANIC_HOURLY_WAGE: i64 = 1_000;
#[cfg(test)]
const MAINTENANCE_REPAIR_COST: i64 = 2_500;
const MAINTENANCE_DECAY_INTERVAL_MINUTES: u32 = 30;
const MAINTENANCE_WEAR_PER_INTERVAL: u32 = 8;
const MAINTENANCE_WEAR_PER_SALE: u32 = 2;
const MAINTENANCE_SERVICE_THRESHOLD: u32 = 60;
const MAINTENANCE_FAILURE_THRESHOLD: u32 = 20;
const FEED_DELIVERY_THRESHOLD: u32 = 90;
const MAX_GUEST_HABITATS_PER_VISIT: usize = 3;
const MIN_ENGAGING_HABITAT_WELFARE: u32 = 40;
const GUEST_CONTINUE_MIN_HAPPINESS: u32 = 45;
const GUEST_CONTINUE_MIN_ENERGY: u32 = 35;
const GUEST_CONTINUE_MAX_HUNGER: u32 = 75;
const GUEST_CONTINUE_MAX_THIRST: u32 = 75;
const GUEST_CONTINUE_MIN_VALUE: u32 = 35;
const GUEST_CLEANLINESS_CONCERN_THRESHOLD: u32 = 70;
const VIEWING_DEPTH_TILES: u32 = 3;
const VIEWING_HALF_WIDTH_TILES: u32 = 2;
const VIEWPOINT_PHYSICAL_CAPACITY: u32 = 4;
const VIEWERS_PER_VISIBLE_ANIMAL: u32 = 2;
const VIEWING_PENALTY_INTERVAL_MINUTES: u32 = 5;
const GATE_UNREACHABLE: &str =
    "Gate unreachable: connect a path from the care depot to the service gate";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TileKind {
    Grass,
    /// A path tile. Every occupied path tile is its own route-graph node (#98).
    Path(PathAccess),
    Entrance,
    Habitat(u32),
    Concession(u32),
    AnimalCareDepot,
    /// Authored scenario terrain: never buildable, walkable or demolishable.
    Water,
}

/// Who may walk a path tile (#98). Guests keep to public paths; every staff job may also use
/// staff-only service routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PathAccess {
    Public,
    Staff,
}

impl PathAccess {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "public" => Some(Self::Public),
            "staff" => Some(Self::Staff),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Staff => "staff",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Staff => "staff-only",
        }
    }
}

/// Whose route permissions a path search follows (#98). Guests walk public paths and the
/// entrance; keepers, janitors and mechanics may also walk staff-only paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mover {
    Guest,
    Staff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    x: u32,
    y: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConcessionKind {
    Food,
    Drink,
}

impl ConcessionKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "food" => Some(Self::Food),
            "drink" => Some(Self::Drink),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Concession {
    id: u32,
    x: u32,
    y: u32,
    kind: ConcessionKind,
    definition: Facility,
    sales_today: u32,
    total_sales: u32,
    total_revenue_cents: i64,
    condition: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Keeper {
    id: u32,
    /// The keeper's work zone; `None` leaves the keeper idle and flagged as needing one.
    zone_id: Option<u32>,
    /// Which care jobs and hours the scheduler may give this keeper.
    schedule: care::KeeperSchedule,
    deliveries_completed: u32,
    /// The keeper's tile; the depot tile itself while at the depot.
    x: u32,
    y: u32,
    activity: care::KeeperActivity,
    /// The outside gate tile the keeper entered its habitat through, used to walk back out.
    exit: Option<Position>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Janitor {
    id: u32,
    x: u32,
    y: u32,
    target_litter_id: Option<u32>,
    tasks_completed: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LitterTask {
    id: u32,
    x: u32,
    y: u32,
    created_minute: u64,
    assigned_janitor_id: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mechanic {
    id: u32,
    x: u32,
    y: u32,
    target_maintenance_id: Option<u32>,
    repairs_completed: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaintenanceTask {
    id: u32,
    concession_id: u32,
    created_minute: u64,
    assigned_mechanic_id: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
enum IncomeCategory {
    Admissions,
    Concessions,
}

#[derive(Clone, Copy, Debug)]
enum ExpenseCategory {
    Construction,
    AnimalPurchase,
    HabitatCare,
    AnimalFeed,
    KeeperHiring,
    JanitorHiring,
    MechanicHiring,
    MaintenanceRepair,
    ParkUpkeep,
    KeeperWages,
    JanitorWages,
    MechanicWages,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FinanceLedger {
    admissions_income_cents: i64,
    concession_income_cents: i64,
    construction_expense_cents: i64,
    animal_purchase_expense_cents: i64,
    habitat_care_expense_cents: i64,
    animal_feed_expense_cents: i64,
    keeper_hiring_expense_cents: i64,
    janitor_hiring_expense_cents: i64,
    mechanic_hiring_expense_cents: i64,
    maintenance_repair_expense_cents: i64,
    park_upkeep_expense_cents: i64,
    keeper_wages_expense_cents: i64,
    janitor_wages_expense_cents: i64,
    mechanic_wages_expense_cents: i64,
}

impl FinanceLedger {
    fn record_income(&mut self, category: IncomeCategory, cents: i64) -> bool {
        if cents < 0
            || self
                .income_total_cents()
                .checked_add(cents)
                .is_none_or(|total| total > content::MAX_CENTS / 2)
        {
            return false;
        }
        match category {
            IncomeCategory::Admissions => self.admissions_income_cents += cents,
            IncomeCategory::Concessions => self.concession_income_cents += cents,
        }
        true
    }

    fn record_expense(&mut self, category: ExpenseCategory, cents: i64) -> bool {
        if cents < 0
            || self
                .expense_total_cents()
                .checked_add(cents)
                .is_none_or(|total| total > content::MAX_CENTS / 2)
        {
            return false;
        }
        match category {
            ExpenseCategory::Construction => self.construction_expense_cents += cents,
            ExpenseCategory::AnimalPurchase => self.animal_purchase_expense_cents += cents,
            ExpenseCategory::HabitatCare => self.habitat_care_expense_cents += cents,
            ExpenseCategory::AnimalFeed => self.animal_feed_expense_cents += cents,
            ExpenseCategory::KeeperHiring => self.keeper_hiring_expense_cents += cents,
            ExpenseCategory::JanitorHiring => self.janitor_hiring_expense_cents += cents,
            ExpenseCategory::MechanicHiring => self.mechanic_hiring_expense_cents += cents,
            ExpenseCategory::MaintenanceRepair => self.maintenance_repair_expense_cents += cents,
            ExpenseCategory::ParkUpkeep => self.park_upkeep_expense_cents += cents,
            ExpenseCategory::KeeperWages => self.keeper_wages_expense_cents += cents,
            ExpenseCategory::JanitorWages => self.janitor_wages_expense_cents += cents,
            ExpenseCategory::MechanicWages => self.mechanic_wages_expense_cents += cents,
        }
        true
    }

    fn income_total_cents(self) -> i64 {
        self.admissions_income_cents + self.concession_income_cents
    }

    fn expense_total_cents(self) -> i64 {
        self.construction_expense_cents
            + self.animal_purchase_expense_cents
            + self.habitat_care_expense_cents
            + self.animal_feed_expense_cents
            + self.keeper_hiring_expense_cents
            + self.janitor_hiring_expense_cents
            + self.mechanic_hiring_expense_cents
            + self.maintenance_repair_expense_cents
            + self.park_upkeep_expense_cents
            + self.keeper_wages_expense_cents
            + self.janitor_wages_expense_cents
            + self.mechanic_wages_expense_cents
    }

    fn profit_cents(self) -> i64 {
        self.income_total_cents() - self.expense_total_cents()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HabitatOrientation {
    Horizontal,
    Vertical,
}

impl HabitatOrientation {
    fn from_code(value: u8) -> Self {
        match value {
            1 => Self::Vertical,
            _ => Self::Horizontal,
        }
    }

    fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Horizontal => (LEGACY_HABITAT_WIDTH, LEGACY_HABITAT_HEIGHT),
            Self::Vertical => (LEGACY_HABITAT_HEIGHT, LEGACY_HABITAT_WIDTH),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FenceSide {
    North,
    East,
    South,
    West,
}

impl FenceSide {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "north" => Some(Self::North),
            "east" => Some(Self::East),
            "south" => Some(Self::South),
            "west" => Some(Self::West),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FenceSegment {
    x: u32,
    y: u32,
    side: FenceSide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ViewingSpot {
    position: Position,
    side: FenceSide,
    visible_animals: u32,
    capacity: u32,
    occupancy: u32,
}

type ViewingOccupancy = HashMap<(u32, u32), u32>;

impl ViewingSpot {
    fn available_capacity(self) -> u32 {
        self.capacity.saturating_sub(self.occupancy)
    }

    fn crowded(self) -> bool {
        self.capacity > 0 && self.occupancy > self.capacity
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WelfareProfile {
    minimum_social_group: u32,
    space_per_animal: u32,
}

impl Species {
    #[cfg(test)]
    fn parse(value: &str) -> Option<Self> {
        Catalog::parse(content::DEFAULT_DOCUMENT)
            .expect("valid built-in catalog")
            .species
            .into_iter()
            .find(|species| species.key == value)
    }
    fn key(&self) -> &str {
        &self.key
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn purchase_cost(&self) -> i64 {
        self.purchase_cost_cents
    }
    fn appeal(&self) -> u32 {
        self.appeal
    }
    fn welfare_profile(&self) -> WelfareProfile {
        WelfareProfile {
            minimum_social_group: self.minimum_social_group,
            space_per_animal: self.space_per_animal,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Habitat {
    id: u32,
    /// The enclosed tiles, row-major: one connected enclosure without holes (#95).
    tiles: Vec<Position>,
    /// The fence type it is enclosed with, owned like its species definition.
    fence: content::FenceType,
    species: Option<Species>,
    /// The individual animals living here, in arrival order (#96); their count is the habitat's.
    animals: Vec<animal::Animal>,
    welfare: u32,
    food: u32,
    water: u32,
    cleanliness: u32,
    /// Terrain, elevation, shelters and enrichment (#97).
    environment: environment::Environment,
    /// Instantiated from the species defaults on first adoption, then owned by the habitat.
    care_schedule: Option<care::CareSchedule>,
    /// The fence segment keepers walk through; its outside tile must stay unbuilt.
    service_gate: FenceSegment,
}

impl Habitat {
    fn area(&self) -> u32 {
        u32::try_from(self.tiles.len()).unwrap_or(u32::MAX)
    }

    fn contains(&self, tile: Position) -> bool {
        shape::contains(&self.tiles, tile)
    }

    fn capacity(&self) -> u32 {
        (self.area() / 3).clamp(2, 12)
    }

    fn fence_length(&self) -> u32 {
        u32::try_from(self.fence_segments().len()).unwrap_or(u32::MAX)
    }

    /// The fence clockwise from the north side of the first row-major tile.
    fn fence_segments(&self) -> Vec<FenceSegment> {
        shape::clockwise_fence(&self.tiles).unwrap_or_default()
    }

    fn appeal(&self) -> u32 {
        self.species.as_ref().map_or(0, |species| {
            species.appeal().saturating_mul(self.animal_count().max(1))
        })
    }

    fn social_score(&self) -> u32 {
        let Some(species) = &self.species else {
            return 100;
        };
        if !self.stocked() {
            return 100;
        }

        let minimum_group = species.welfare_profile().minimum_social_group.max(1);
        self.animal_count()
            .saturating_mul(100)
            .checked_div(minimum_group)
            .unwrap_or(100)
            .min(100)
    }

    fn space_score(&self) -> u32 {
        self.space_score_with(&self.assessment())
    }

    /// Space counts only usable dry land reachable from the gate (#97).
    fn space_score_with(&self, assessment: &environment::Assessment) -> u32 {
        let Some(species) = &self.species else {
            return 100;
        };
        if !self.stocked() {
            return 100;
        }

        let available_space = assessment.usable_land;
        let required_space = self
            .animal_count()
            .saturating_mul(species.welfare_profile().space_per_animal);
        if required_space <= available_space {
            100
        } else {
            available_space
                .saturating_mul(100)
                .checked_div(required_space)
                .unwrap_or(0)
                .min(100)
        }
    }

    fn food_score(&self) -> u32 {
        if !self.stocked() { 100 } else { self.food }
    }

    fn water_score(&self) -> u32 {
        if !self.stocked() { 100 } else { self.water }
    }

    fn cleanliness_score(&self) -> u32 {
        if !self.stocked() {
            100
        } else {
            self.cleanliness
        }
    }

    /// The weighted parts of [`Habitat::welfare_parts`]: social group and space count double;
    /// food, drinking water, cleanliness, shelter, enrichment, ground cover, shallow and deep
    /// water count once each.
    fn welfare_target(&self) -> u32 {
        Self::target_of(&self.welfare_parts())
    }

    fn welfare_status(&self, staffing: Option<&str>, gate_reachable: bool) -> String {
        let Some(species) = &self.species else {
            return "Ready for animals".to_owned();
        };
        let social = self.social_score();
        let space = self.space_score();
        let minimum_group = species.welfare_profile().minimum_social_group;

        match (social < 100, space < 100) {
            (true, true) => "Social group is too small and habitat space is crowded".to_owned(),
            (true, false) => format!("Social group needs at least {minimum_group} animals"),
            (false, true) => "Habitat space is crowded for this group".to_owned(),
            (false, false) => self.care_status(staffing, gate_reachable),
        }
    }

    /// `staffing` is why no keeper can serve this habitat at all, if so.
    fn care_status(&self, staffing: Option<&str>, gate_reachable: bool) -> String {
        if let Some(problem) = staffing {
            return problem.to_owned();
        }
        if !gate_reachable {
            return GATE_UNREACHABLE.to_owned();
        }
        if !self.stocked() {
            return "Zone keepers ready; care starts with the first animals".to_owned();
        }
        if self.food < 40 {
            return "Food is running low".to_owned();
        }
        if self.water < 40 {
            return "Water is running low".to_owned();
        }
        if self.cleanliness < 50 {
            return "Habitat needs cleaning".to_owned();
        }
        if let Some(problem) = self.environment_problem() {
            return problem;
        }
        "Food, water, shelter, and cleanliness are healthy".to_owned()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GuestState {
    Arriving,
    WalkingToHabitat,
    Viewing,
    WalkingToExit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guest {
    id: u32,
    x: u32,
    y: u32,
    happiness: u32,
    energy: u32,
    hunger: u32,
    thirst: u32,
    value_perception: u32,
    minutes_in_park: u32,
    target_habitat: u32,
    state: GuestState,
    route: Vec<Position>,
    route_index: usize,
    viewing_minutes: u32,
    arrival_steps: u8,
    bought_food: bool,
    bought_drink: bool,
    visited_habitats: Vec<u32>,
    cleanliness_concern: bool,
}

impl Guest {
    fn wants_another_habitat(&self) -> bool {
        self.visited_habitats.len() < MAX_GUEST_HABITATS_PER_VISIT
            && self.happiness >= GUEST_CONTINUE_MIN_HAPPINESS
            && self.energy >= GUEST_CONTINUE_MIN_ENERGY
            && self.hunger <= GUEST_CONTINUE_MAX_HUNGER
            && self.thirst <= GUEST_CONTINUE_MAX_THIRST
            && self.value_perception >= GUEST_CONTINUE_MIN_VALUE
    }
}

fn normalized_rect(ax: u32, ay: u32, bx: u32, by: u32) -> (u32, u32, u32, u32) {
    let x = ax.min(bx);
    let y = ay.min(by);
    let width = ax.abs_diff(bx).saturating_add(1);
    let height = ay.abs_diff(by).saturating_add(1);
    (x, y, width, height)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GameState {
    /// Saved beside the park in the envelope, not inside it.
    #[serde(skip, default = "default_catalog")]
    catalog: Arc<Catalog>,
    width: u32,
    height: u32,
    tiles: Vec<TileKind>,
    entrance: Position,
    depot: Position,
    /// The immutable identity of the scenario this park was created from.
    scenario: scenario::ScenarioIdentity,
    unlocks: scenario::Unlocks,
    objectives: Vec<scenario::ObjectiveProgress>,
    habitats: Vec<Habitat>,
    concessions: Vec<Concession>,
    keepers: Vec<Keeper>,
    janitors: Vec<Janitor>,
    litter: Vec<LitterTask>,
    mechanics: Vec<Mechanic>,
    maintenance: Vec<MaintenanceTask>,
    guests: Vec<Guest>,
    cash_cents: i64,
    feed_crates: u32,
    day: u32,
    minute_of_day: u32,
    next_habitat_id: u32,
    next_concession_id: u32,
    next_keeper_id: u32,
    next_janitor_id: u32,
    next_litter_id: u32,
    next_mechanic_id: u32,
    next_maintenance_id: u32,
    next_guest_id: u32,
    /// Animal IDs are park-wide and never reused (#96).
    next_animal_id: u32,
    /// Shelter and enrichment IDs are park-wide and never reused (#97).
    next_habitat_object_id: u32,
    spawn_accumulator: u32,
    upkeep_accumulator: u32,
    movement_accumulator: u32,
    finance_today: FinanceLedger,
    finance_previous: Option<FinanceLedger>,
    /// Simulated minutes since this park session started (the diagnostics clock).
    elapsed_minutes: u64,
    /// Domain events produced by the latest command or advance, drained by the recorder.
    #[serde(skip)]
    pending_events: Vec<session::DomainEvent>,
    care_jobs: Vec<care::CareJob>,
    next_care_job_id: u32,
    work_zones: Vec<zone::WorkZone>,
    next_work_zone_id: u32,
    /// Onboarding progress; it observes the park and never changes simulation rules.
    tutorial: tutorial::Tutorial,
    /// The park's current problems (#94); saved so first occurrences survive a reload.
    alerts: Vec<alerts::Alert>,
    /// Bounded construction undo/redo; session state, never saved (#92).
    #[serde(skip)]
    construction: construction::History,
    #[serde(skip)]
    work: WorkCounters,
}

/// Diagnostic work counters. They measure effort, not park state, so their Debug output is
/// constant and cannot make an unchanged park look mutated.
#[derive(Clone, Default)]
struct WorkCounters {
    guest_updates: u64,
    path_searches: Cell<u64>,
    /// Path-graph edits this session (#98): building, widening, reclassifying or removing path.
    topology_revision: u64,
    /// Guest routes and staff assignments a topology edit invalidated and re-planned.
    route_invalidations: u64,
}

impl std::fmt::Debug for WorkCounters {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WorkCounters")
    }
}

fn default_catalog() -> Arc<Catalog> {
    Arc::new(Catalog::parse(content::DEFAULT_DOCUMENT).expect("valid built-in catalog"))
}

impl Default for GameState {
    /// The built-in empty starter park on the built-in content.
    fn default() -> Self {
        scenario::Scenario::built_in(scenario::DEFAULT_SCENARIO_ID)
            .expect("the default scenario is built in")
            .create(default_catalog())
            .expect("the built-in default scenario is valid")
    }
}

impl GameState {
    /// An unbuilt park of grass with only its entrance and depot; scenarios build on it.
    fn blank(
        catalog: Arc<Catalog>,
        width: u32,
        height: u32,
        entrance: Position,
        depot: Position,
        scenario: scenario::ScenarioIdentity,
    ) -> Self {
        let mut state = Self {
            catalog,
            width,
            height,
            tiles: vec![TileKind::Grass; (width * height) as usize],
            entrance,
            depot,
            scenario,
            unlocks: scenario::Unlocks::default(),
            objectives: Vec::new(),
            habitats: Vec::new(),
            concessions: Vec::new(),
            keepers: Vec::new(),
            janitors: Vec::new(),
            litter: Vec::new(),
            mechanics: Vec::new(),
            maintenance: Vec::new(),
            guests: Vec::new(),
            cash_cents: 5_000_000,
            feed_crates: 0,
            day: 1,
            minute_of_day: 9 * 60,
            next_habitat_id: 1,
            next_concession_id: 1,
            next_keeper_id: 1,
            next_janitor_id: 1,
            next_litter_id: 1,
            next_mechanic_id: 1,
            next_maintenance_id: 1,
            next_guest_id: 1,
            next_animal_id: 1,
            next_habitat_object_id: 1,
            spawn_accumulator: 0,
            upkeep_accumulator: 0,
            movement_accumulator: 0,
            finance_today: FinanceLedger::default(),
            finance_previous: None,
            elapsed_minutes: 0,
            pending_events: Vec::new(),
            care_jobs: Vec::new(),
            next_care_job_id: 1,
            work_zones: Vec::new(),
            next_work_zone_id: 1,
            tutorial: tutorial::Tutorial::off(),
            alerts: Vec::new(),
            construction: construction::History::default(),
            work: WorkCounters::default(),
        };

        state.set_tile(entrance.x, entrance.y, TileKind::Entrance);
        state.set_tile(depot.x, depot.y, TileKind::AnimalCareDepot);
        state
    }

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then_some((y * self.width + x) as usize)
    }

    fn tile(&self, x: u32, y: u32) -> Option<TileKind> {
        self.index(x, y).map(|index| self.tiles[index])
    }

    fn set_tile(&mut self, x: u32, y: u32, kind: TileKind) {
        if let Some(index) = self.index(x, y) {
            self.tiles[index] = kind;
        }
    }

    fn spend(&mut self, cents: i64, category: ExpenseCategory) -> Result<(), &'static str> {
        if self.cash_cents < cents {
            return Err("Not enough cash");
        }
        let Some(cash) = self
            .cash_cents
            .checked_sub(cents)
            .filter(|cash| (-content::MAX_CENTS..=content::MAX_CENTS).contains(cash))
        else {
            return Err("Cash exceeds the exact financial range");
        };
        let mut ledger = self.finance_today;
        if !ledger.record_expense(category, cents) {
            return Err("Expense exceeds the exact financial range");
        }
        self.cash_cents = cash;
        self.finance_today = ledger;
        Ok(())
    }

    fn earn(&mut self, cents: i64, category: IncomeCategory) -> bool {
        let Some(cash) = self
            .cash_cents
            .checked_add(cents)
            .filter(|cash| (-content::MAX_CENTS..=content::MAX_CENTS).contains(cash))
        else {
            return false;
        };
        let mut ledger = self.finance_today;
        if !ledger.record_income(category, cents) {
            return false;
        }
        self.cash_cents = cash;
        self.finance_today = ledger;
        true
    }

    fn adopt(&mut self, habitat_id: u32, species_name: &str) -> ActionResult {
        let Some(species) = self
            .catalog
            .species
            .iter()
            .find(|species| species.key == species_name)
            .cloned()
        else {
            return ActionResult::error("Unknown species");
        };
        if !self.unlocks.species_unlocked(&species.key) {
            return ActionResult::error(format!(
                "{} is not available in this scenario",
                species.label()
            ));
        }
        let Some(index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
        else {
            return ActionResult::error("Select a habitat first");
        };

        let habitat = &self.habitats[index];
        if habitat.animal_count() >= habitat.capacity() {
            return ActionResult::error("That habitat is at capacity");
        }
        if habitat
            .species
            .as_ref()
            .is_some_and(|current| current.key != species.key)
        {
            return ActionResult::error("Each habitat currently keeps one species");
        }
        if habitat.fence.height_cm < species.minimum_fence_height_cm {
            return ActionResult::error(format!(
                "{} needs a fence at least {} cm high; habitat #{habitat_id}'s {} is {} cm. Upgrade its fence first",
                species.label(),
                species.minimum_fence_height_cm,
                habitat.fence.label.to_lowercase(),
                habitat.fence.height_cm
            ));
        }
        if let Some(problem) = self.zone_staffing_problem(habitat) {
            return ActionResult::error(format!("Staff this habitat before adopting: {problem}"));
        }

        if let Err(message) = self.spend(species.purchase_cost(), ExpenseCategory::AnimalPurchase) {
            return ActionResult::error(message);
        }
        let now = self.absolute_minute();
        let habitat = &mut self.habitats[index];
        if habitat.care_schedule.is_none() {
            habitat.care_schedule = Some(care::CareSchedule::instantiate(&species.care, now));
        }
        habitat.species = Some(species.clone());
        let id = self.add_animal(index, &species.key);
        let name = self.habitats[index]
            .animals
            .last()
            .map_or("", |animal| animal.name.as_str());
        ActionResult::ok(format!(
            "{} adopted into habitat #{habitat_id}: {name} (#{id})",
            species.label()
        ))
    }

    fn absolute_minute(&self) -> u64 {
        u64::from(self.day.saturating_sub(1))
            .saturating_mul(24 * 60)
            .saturating_add(u64::from(self.minute_of_day))
    }

    fn buy_animal_feed(&mut self) -> ActionResult {
        if let Err(message) = self.spend(
            self.catalog.balance.feed_batch_cents,
            ExpenseCategory::AnimalFeed,
        ) {
            return ActionResult::error(message);
        }
        self.feed_crates = self.feed_crates.saturating_add(FEED_BATCH_CRATES);
        ActionResult::ok(format!(
            "Bought {FEED_BATCH_CRATES} feed crates for the animal-care depot"
        ))
    }

    fn hire_keeper(&mut self) -> ActionResult {
        if let Err(message) = self.spend(
            self.catalog.balance.staff.keeper.hire_cents,
            ExpenseCategory::KeeperHiring,
        ) {
            return ActionResult::error(message);
        }
        let id = self.next_keeper_id;
        self.next_keeper_id = self.next_keeper_id.saturating_add(1);
        self.keepers.push(Keeper {
            id,
            zone_id: None,
            schedule: care::KeeperSchedule::default(),
            deliveries_completed: 0,
            x: self.depot.x,
            y: self.depot.y,
            activity: care::KeeperActivity::AtDepot,
            exit: None,
        });
        ActionResult::ok(format!("Keeper #{id} hired at the animal-care depot"))
    }

    fn depot_staff_spawn(&self) -> Option<Position> {
        let access_tiles = self.neighbors(Position {
            x: self.depot.x,
            y: self.depot.y,
        });
        self.path_to_any(
            Mover::Staff,
            Position {
                x: self.entrance.x,
                y: self.entrance.y,
            },
            &access_tiles,
        )
        .and_then(|route| route.last().copied())
    }

    fn hire_janitor(&mut self) -> ActionResult {
        let Some(spawn) = self.depot_staff_spawn() else {
            return ActionResult::error(
                "Connect the central operations depot to the park entrance before hiring janitors",
            );
        };
        if let Err(message) = self.spend(
            self.catalog.balance.staff.janitor.hire_cents,
            ExpenseCategory::JanitorHiring,
        ) {
            return ActionResult::error(message);
        }
        let id = self.next_janitor_id;
        self.next_janitor_id = self.next_janitor_id.saturating_add(1);
        self.janitors.push(Janitor {
            id,
            x: spawn.x,
            y: spawn.y,
            target_litter_id: None,
            tasks_completed: 0,
        });
        ActionResult::ok(format!(
            "Janitor #{id} hired at the central operations depot"
        ))
    }

    fn hire_mechanic(&mut self) -> ActionResult {
        let Some(spawn) = self.depot_staff_spawn() else {
            return ActionResult::error(
                "Connect the central operations depot to the park entrance before hiring mechanics",
            );
        };
        if let Err(message) = self.spend(
            self.catalog.balance.staff.mechanic.hire_cents,
            ExpenseCategory::MechanicHiring,
        ) {
            return ActionResult::error(message);
        }
        let id = self.next_mechanic_id;
        self.next_mechanic_id = self.next_mechanic_id.saturating_add(1);
        self.mechanics.push(Mechanic {
            id,
            x: spawn.x,
            y: spawn.y,
            target_maintenance_id: None,
            repairs_completed: 0,
        });
        ActionResult::ok(format!(
            "Mechanic #{id} hired at the central operations depot"
        ))
    }

    /// Why no keeper can ever serve this habitat's jobs, if so: its gate lies outside every
    /// work zone, or nobody works in that zone. There is no park-wide staffing fallback.
    fn zone_staffing_problem(&self, habitat: &Habitat) -> Option<String> {
        let Some(zone_id) = self.habitat_zone(habitat) else {
            return Some(
                "No work zone: the service gate is outside every work zone; draw a zone around it"
                    .to_owned(),
            );
        };
        (self.zone_keeper_count(zone_id) == 0).then(|| {
            format!("No keeper works in zone #{zone_id}; assign one in the zone inspector")
        })
    }

    /// The zone staffing problem, or else a dangerous-species crew the zone cannot field:
    /// fewer keepers work there than each of the habitat's jobs needs at once.
    fn staffing_problem(&self, habitat: &Habitat) -> Option<String> {
        if let Some(problem) = self.zone_staffing_problem(habitat) {
            return Some(problem);
        }
        let zone_id = self.habitat_zone(habitat)?;
        let required = habitat.care_schedule.as_ref()?.required_keepers as usize;
        let available = self.zone_keeper_count(zone_id);
        (available < required).then(|| {
            format!(
                "Dangerous animals: every job needs {required} keepers together, but only {available} works in zone #{zone_id}"
            )
        })
    }

    fn refill_water(&mut self, habitat_id: u32) -> ActionResult {
        let Some(index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
        else {
            return ActionResult::error("Select a habitat first");
        };
        if self.habitats[index].water >= 100 {
            return ActionResult::ok("Water is already full");
        }
        if let Err(message) = self.spend(
            self.catalog.balance.habitat_care.water_refill_cents,
            ExpenseCategory::HabitatCare,
        ) {
            return ActionResult::error(message);
        }
        self.habitats[index].water = 100;
        ActionResult::ok(format!("Habitat #{habitat_id} water refilled"))
    }

    fn clean_habitat(&mut self, habitat_id: u32) -> ActionResult {
        let Some(index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
        else {
            return ActionResult::error("Select a habitat first");
        };
        if self.habitats[index].cleanliness >= 100 {
            return ActionResult::ok("Habitat is already clean");
        }
        if let Err(message) = self.spend(
            self.catalog.balance.habitat_care.cleaning_cents,
            ExpenseCategory::HabitatCare,
        ) {
            return ActionResult::error(message);
        }
        self.habitats[index].cleanliness = 100;
        ActionResult::ok(format!("Habitat #{habitat_id} cleaned"))
    }

    fn tick(&mut self, minutes: u32) {
        for _ in 0..minutes {
            self.elapsed_minutes += 1;
            self.minute_of_day += 1;
            if self.minute_of_day >= 24 * 60 {
                self.minute_of_day = 0;
                self.day += 1;
                self.finance_previous = Some(self.finance_today);
                self.finance_today = FinanceLedger::default();
                // Construction undo spans one in-game day; refunds never cross the ledger.
                self.construction.clear();
                for stand in &mut self.concessions {
                    stand.sales_today = 0;
                }
            }

            self.spawn_accumulator += 1;
            self.upkeep_accumulator += 1;
            self.movement_accumulator += 1;

            if self.spawn_accumulator >= 24 {
                self.spawn_accumulator = 0;
                self.try_spawn_guest();
            }
            if self.upkeep_accumulator >= 60 {
                self.upkeep_accumulator = 0;
                self.charge_upkeep();
            }
            if self
                .minute_of_day
                .is_multiple_of(CARE_DECAY_INTERVAL_MINUTES)
            {
                self.advance_habitat_care();
            }
            if self
                .minute_of_day
                .is_multiple_of(MAINTENANCE_DECAY_INTERVAL_MINUTES)
            {
                self.advance_concession_maintenance();
            }
            self.advance_care_jobs();

            if self
                .minute_of_day
                .is_multiple_of(animal::STEP_INTERVAL_MINUTES)
            {
                self.advance_animals();
            }
            self.advance_animal_welfare();
            self.advance_guest_needs();

            if self.movement_accumulator >= 3 {
                self.movement_accumulator = 0;
                self.advance_guest_movement();
                self.advance_janitor_work();
                self.advance_mechanic_work();
                self.advance_keeper_movement();
            }

            self.advance_viewing();
            self.update_alerts();
        }
    }

    fn emit(&mut self, kind: &'static str, entity_id: Option<u32>) {
        self.pending_events.push(session::DomainEvent {
            tick: self.elapsed_minutes,
            kind,
            entity_id,
        });
    }

    /// Cheap (linear in entities) consistency checks run before every recorded advance.
    /// A violation means authoritative state is corrupt and the session must freeze.
    fn invariant_violation(&self) -> Option<String> {
        if !(-content::MAX_CENTS..=content::MAX_CENTS).contains(&self.cash_cents) {
            return Some(format!(
                "cash {} is outside the representable range",
                self.cash_cents
            ));
        }
        if self.tiles.len() != (self.width as usize) * (self.height as usize) {
            return Some("tile grid does not match the park dimensions".to_owned());
        }
        if let Some(violation) = self.animal_violation() {
            return Some(violation);
        }
        fn ordered(name: &str, ids: impl Iterator<Item = u32>, next: u32) -> Option<String> {
            let mut previous = 0;
            for id in ids {
                if id <= previous || id >= next {
                    return Some(format!(
                        "{name} id {id} is duplicated, out of order or not yet allocated (next {next})"
                    ));
                }
                previous = id;
            }
            None
        }
        ordered(
            "habitat",
            self.habitats.iter().map(|h| h.id),
            self.next_habitat_id,
        )
        .or_else(|| {
            ordered(
                "concession",
                self.concessions.iter().map(|c| c.id),
                self.next_concession_id,
            )
        })
        .or_else(|| {
            ordered(
                "keeper",
                self.keepers.iter().map(|k| k.id),
                self.next_keeper_id,
            )
        })
        .or_else(|| {
            ordered(
                "janitor",
                self.janitors.iter().map(|j| j.id),
                self.next_janitor_id,
            )
        })
        .or_else(|| {
            ordered(
                "mechanic",
                self.mechanics.iter().map(|m| m.id),
                self.next_mechanic_id,
            )
        })
        .or_else(|| {
            ordered(
                "care job",
                self.care_jobs.iter().map(|j| j.id),
                self.next_care_job_id,
            )
        })
        .or_else(|| {
            ordered(
                "guest",
                self.guests.iter().map(|g| g.id),
                self.next_guest_id,
            )
        })
        .or_else(|| {
            ordered(
                "work zone",
                self.work_zones.iter().map(|z| z.id),
                self.next_work_zone_id,
            )
        })
        .or_else(|| self.work_zone_violation())
    }

    fn advance_habitat_care(&mut self) {
        for habitat in &mut self.habitats {
            if !habitat.stocked() {
                continue;
            }
            habitat.food = habitat.food.saturating_sub(habitat.animal_count());
            habitat.water = habitat.water.saturating_sub(habitat.animal_count());
            let waste = habitat.animal_count().saturating_add(1) / 2;
            habitat.cleanliness = habitat.cleanliness.saturating_sub(waste.max(1));
        }
    }

    fn advance_animal_welfare(&mut self) {
        for habitat in &mut self.habitats {
            if !habitat.stocked() {
                continue;
            }
            let target = habitat.welfare_target();
            if habitat.welfare < target {
                habitat.welfare = habitat.welfare.saturating_add(1).min(target);
            } else if habitat.welfare > target {
                habitat.welfare = habitat.welfare.saturating_sub(1).max(target);
            }
        }
    }

    fn try_spawn_guest(&mut self) {
        let start = Position {
            x: self.entrance.x,
            y: self.entrance.y,
        };
        let viewing_occupancy = self.viewing_occupancy();
        let mut candidates: Vec<(u32, Vec<Position>)> = self
            .habitats
            .iter()
            .filter(|habitat| habitat.stocked())
            .filter_map(|habitat| {
                self.viewing_route_with_occupancy(habitat, start, &viewing_occupancy)
                    .map(|route| (habitat.id, route))
            })
            .collect();
        if candidates.is_empty() {
            return;
        }

        // Choose only among reachable attractions and reuse the selected search result.
        let choice = (self.next_guest_id as usize) % candidates.len();
        let (target_habitat, route) = candidates.swap_remove(choice);

        if self.next_guest_id == u32::MAX
            || !self.earn(
                self.catalog.balance.admission_price_cents,
                IncomeCategory::Admissions,
            )
        {
            return;
        }
        self.guests.push(Guest {
            id: self.next_guest_id,
            x: start.x,
            y: start.y,
            happiness: 78,
            energy: 90,
            hunger: 18,
            thirst: 16,
            value_perception: 68,
            minutes_in_park: 0,
            target_habitat,
            state: GuestState::Arriving,
            route,
            route_index: 0,
            viewing_minutes: 0,
            arrival_steps: 2,
            bought_food: false,
            bought_drink: false,
            visited_habitats: Vec::new(),
            cleanliness_concern: false,
        });
        self.emit("guest_arrived", Some(self.next_guest_id));
        self.next_guest_id += 1;
    }

    fn advance_guest_needs(&mut self) {
        self.work.guest_updates += self.guests.len() as u64;
        let park_cleanliness = self.park_cleanliness();
        let cleanliness_concern = park_cleanliness < GUEST_CLEANLINESS_CONCERN_THRESHOLD;
        let cleanliness_penalty = GUEST_CLEANLINESS_CONCERN_THRESHOLD
            .saturating_sub(park_cleanliness)
            .div_ceil(20)
            .max(1);

        for guest in &mut self.guests {
            guest.minutes_in_park += 1;
            guest.cleanliness_concern = cleanliness_concern;
            if guest.minutes_in_park % 4 == 0 {
                guest.energy = guest.energy.saturating_sub(1);
            }
            if guest.minutes_in_park % 3 == 0 {
                guest.hunger = guest.hunger.saturating_add(1).min(100);
            }
            if guest.minutes_in_park % 2 == 0 {
                guest.thirst = guest.thirst.saturating_add(1).min(100);
            }
            if guest.minutes_in_park % 10 == 0 {
                guest.value_perception = guest.value_perception.saturating_sub(1);
                if cleanliness_concern {
                    guest.happiness = guest.happiness.saturating_sub(cleanliness_penalty);
                    guest.value_perception = guest.value_perception.saturating_sub(1);
                }
            }
            if guest.minutes_in_park % 5 == 0
                && (guest.hunger >= 60 || guest.thirst >= 60 || guest.energy <= 35)
            {
                guest.happiness = guest.happiness.saturating_sub(1);
            }
        }
    }

    fn habitat_experience_bonus(&self, habitat_id: u32) -> u32 {
        self.habitats
            .iter()
            .find(|habitat| habitat.id == habitat_id)
            .map_or(0, |habitat| {
                4 + habitat.welfare / 20 + habitat.appeal().min(300) / 30
            })
    }

    fn next_engaging_habitat(
        &self,
        guest: &Guest,
        start: Position,
        viewing_occupancy: &ViewingOccupancy,
    ) -> Option<(u32, Vec<Position>)> {
        let mut candidates: Vec<&Habitat> = self
            .habitats
            .iter()
            .filter(|habitat| habitat.stocked())
            .filter(|habitat| habitat.welfare >= MIN_ENGAGING_HABITAT_WELFARE)
            .filter(|habitat| !guest.visited_habitats.contains(&habitat.id))
            .collect();

        candidates.sort_by(|left, right| {
            let left_score = left.appeal().saturating_add(left.welfare.saturating_mul(2));
            let right_score = right
                .appeal()
                .saturating_add(right.welfare.saturating_mul(2));
            right_score
                .cmp(&left_score)
                .then_with(|| left.id.cmp(&right.id))
        });

        candidates.into_iter().find_map(|habitat| {
            self.viewing_route_with_occupancy(habitat, start, viewing_occupancy)
                .map(|route| (habitat.id, route))
        })
    }

    /// Every path-graph edit (building, widening, reclassifying or removing path, and the
    /// buildings that change which tiles are walkable) ends here (#98). It advances the
    /// topology revision and re-plans exactly the routes and staff assignments the edit broke;
    /// valid routes are kept so unrelated construction never restarts a journey.
    fn topology_changed(&mut self) {
        self.work.topology_revision = self.work.topology_revision.saturating_add(1);
        self.refresh_guest_routes();
        self.release_unreachable_janitor_assignments();
        self.release_unreachable_mechanic_assignments();
    }

    fn note_route_invalidated(&mut self) {
        self.work.route_invalidations = self.work.route_invalidations.saturating_add(1);
    }

    fn refresh_guest_routes(&mut self) {
        // Topology edits, not simulation ticks, retry blocked routes. Preserve valid
        // cached routes so unrelated construction does not restart guest journeys.
        let viewing_occupancy = self.viewing_occupancy();
        for index in 0..self.guests.len() {
            let guest = &self.guests[index];
            if guest.state == GuestState::Viewing {
                continue;
            }
            let remaining = guest.route.get(guest.route_index..).unwrap_or_default();
            let still_walkable = !remaining.is_empty()
                && remaining
                    .iter()
                    .all(|position| self.walkable(Mover::Guest, *position));
            if still_walkable {
                continue;
            }
            if !remaining.is_empty() {
                self.note_route_invalidated();
            }
            let guest = &self.guests[index];
            let start = Position {
                x: guest.x,
                y: guest.y,
            };
            let route = match guest.state {
                GuestState::Arriving | GuestState::WalkingToHabitat => self
                    .habitats
                    .iter()
                    .find(|habitat| habitat.id == guest.target_habitat)
                    .and_then(|habitat| {
                        self.viewing_route_with_occupancy(habitat, start, &viewing_occupancy)
                    }),
                GuestState::WalkingToExit => self.path_between(
                    Mover::Guest,
                    start,
                    Position {
                        x: self.entrance.x,
                        y: self.entrance.y,
                    },
                ),
                GuestState::Viewing => unreachable!("viewing guests have no active walking route"),
            };
            let guest = &mut self.guests[index];
            // An empty route means waiting in place, never having arrived.
            guest.route = route.unwrap_or_default();
            guest.route_index = 0;
        }
    }

    fn advance_guest_movement(&mut self) {
        let mut leave_ids = Vec::new();

        for index in 0..self.guests.len() {
            let state = self.guests[index].state;
            if matches!(state, GuestState::Viewing) {
                continue;
            }
            if matches!(state, GuestState::Arriving) {
                if self.guests[index].arrival_steps > 0 {
                    self.guests[index].arrival_steps -= 1;
                } else {
                    self.guests[index].state = GuestState::WalkingToHabitat;
                }
                continue;
            }

            if self.guests[index].route.is_empty() {
                continue;
            }
            let next_index = self.guests[index].route_index + 1;
            if next_index < self.guests[index].route.len() {
                let position = self.guests[index].route[next_index];
                if !self.walkable(Mover::Guest, position) {
                    self.guests[index].route.clear();
                    self.guests[index].route_index = 0;
                    continue;
                }
                self.guests[index].route_index = next_index;
                self.guests[index].x = position.x;
                self.guests[index].y = position.y;
                continue;
            }

            match state {
                GuestState::WalkingToHabitat => {
                    let target_habitat = self.guests[index].target_habitat;
                    let experience_bonus = self.habitat_experience_bonus(target_habitat);
                    let guest = &mut self.guests[index];
                    if !guest.visited_habitats.contains(&target_habitat) {
                        guest.visited_habitats.push(target_habitat);
                    }
                    guest.state = GuestState::Viewing;
                    guest.viewing_minutes = 24 + (experience_bonus / 2).min(10);
                    guest.happiness = guest.happiness.saturating_add(experience_bonus).min(100);
                    guest.value_perception = guest
                        .value_perception
                        .saturating_add(experience_bonus / 2)
                        .min(100);
                }
                GuestState::WalkingToExit => leave_ids.push(self.guests[index].id),
                GuestState::Arriving | GuestState::Viewing => {}
            }
        }

        if !leave_ids.is_empty() {
            for id in &leave_ids {
                self.emit("guest_left", Some(*id));
            }
            self.guests.retain(|guest| !leave_ids.contains(&guest.id));
        }
        self.serve_concessions();
    }

    fn serve_concessions(&mut self) {
        for guest_index in 0..self.guests.len() {
            let (position, state, hunger, thirst, bought_food, bought_drink) = {
                let guest = &self.guests[guest_index];
                (
                    Position {
                        x: guest.x,
                        y: guest.y,
                    },
                    guest.state,
                    guest.hunger,
                    guest.thirst,
                    guest.bought_food,
                    guest.bought_drink,
                )
            };

            if !matches!(
                state,
                GuestState::WalkingToHabitat | GuestState::WalkingToExit
            ) {
                continue;
            }

            let adjacent_ids: Vec<u32> = self
                .neighbors(position)
                .into_iter()
                .filter_map(|neighbor| match self.tile(neighbor.x, neighbor.y) {
                    Some(TileKind::Concession(id)) => Some(id),
                    _ => None,
                })
                .collect();
            if adjacent_ids.is_empty() {
                continue;
            }

            let drink_choice = (!bought_drink && thirst >= DRINK_BUY_THRESHOLD)
                .then(|| {
                    adjacent_ids.iter().find_map(|id| {
                        self.concessions.iter().position(|stand| {
                            stand.id == *id
                                && stand.kind == ConcessionKind::Drink
                                && stand.condition > MAINTENANCE_FAILURE_THRESHOLD
                        })
                    })
                })
                .flatten();
            let food_choice = (!bought_food && hunger >= FOOD_BUY_THRESHOLD)
                .then(|| {
                    adjacent_ids.iter().find_map(|id| {
                        self.concessions.iter().position(|stand| {
                            stand.id == *id
                                && stand.kind == ConcessionKind::Food
                                && stand.condition > MAINTENANCE_FAILURE_THRESHOLD
                        })
                    })
                })
                .flatten();
            let Some(concession_index) = drink_choice.or(food_choice) else {
                continue;
            };

            let kind = self.concessions[concession_index].kind;
            let degraded =
                self.concessions[concession_index].condition <= MAINTENANCE_SERVICE_THRESHOLD;
            let stand_id = self.concessions[concession_index].id;
            let price = self.concessions[concession_index].definition.price_cents;
            let stand = &self.concessions[concession_index];
            let Some(revenue) = stand
                .total_revenue_cents
                .checked_add(price)
                .filter(|total| *total <= content::MAX_CENTS)
            else {
                continue;
            };
            if stand.sales_today == u32::MAX
                || stand.total_sales == u32::MAX
                || !self.earn(price, IncomeCategory::Concessions)
            {
                continue;
            }
            {
                let stand = &mut self.concessions[concession_index];
                stand.sales_today += 1;
                stand.total_sales += 1;
                stand.total_revenue_cents = revenue;
                stand.condition = stand.condition.saturating_sub(MAINTENANCE_WEAR_PER_SALE);
            }
            self.ensure_maintenance_task(stand_id);
            {
                let guest = &mut self.guests[guest_index];
                let restore = if degraded { 35 } else { 55 };
                match kind {
                    ConcessionKind::Food => {
                        guest.hunger = guest.hunger.saturating_sub(restore);
                        guest.bought_food = true;
                    }
                    ConcessionKind::Drink => {
                        guest.thirst = guest.thirst.saturating_sub(restore);
                        guest.bought_drink = true;
                    }
                }
                let happiness = if degraded { 1 } else { 3 };
                guest.happiness = guest.happiness.saturating_add(happiness).min(100);
                guest.value_perception = guest.value_perception.saturating_add(2).min(100);
            }
            self.add_litter(position);
        }
    }

    fn advance_viewing(&mut self) {
        let viewing_occupancy = self.viewing_occupancy();
        let conditions: Vec<(usize, Option<ViewingSpot>)> = self
            .guests
            .iter()
            .enumerate()
            .filter(|(_, guest)| guest.state == GuestState::Viewing)
            .map(|(index, guest)| {
                (
                    index,
                    self.viewing_spot_for_guest_with_occupancy(guest, &viewing_occupancy),
                )
            })
            .collect();

        let mut decisions = Vec::new();
        for (index, spot) in conditions {
            let guest = &mut self.guests[index];
            if guest
                .minutes_in_park
                .is_multiple_of(VIEWING_PENALTY_INTERVAL_MINUTES)
            {
                if spot.is_some_and(|spot| spot.visible_animals == 0) {
                    guest.happiness = guest.happiness.saturating_sub(2);
                    guest.value_perception = guest.value_perception.saturating_sub(1);
                } else if spot.is_some_and(ViewingSpot::crowded) {
                    guest.happiness = guest.happiness.saturating_sub(1);
                    guest.value_perception = guest.value_perception.saturating_sub(1);
                }
            }

            guest.viewing_minutes = guest.viewing_minutes.saturating_sub(1);
            if guest.viewing_minutes == 0 {
                decisions.push((
                    index,
                    Position {
                        x: guest.x,
                        y: guest.y,
                    },
                ));
            }
        }

        for (index, start) in decisions {
            let next_habitat = {
                let guest = &self.guests[index];
                if guest.wants_another_habitat() {
                    self.next_engaging_habitat(guest, start, &viewing_occupancy)
                } else {
                    None
                }
            };

            if let Some((habitat_id, route)) = next_habitat {
                let guest = &mut self.guests[index];
                guest.target_habitat = habitat_id;
                guest.state = GuestState::WalkingToHabitat;
                guest.route = route;
                guest.route_index = 0;
                continue;
            }

            let exit = Position {
                x: self.entrance.x,
                y: self.entrance.y,
            };
            if let Some(route) = self.path_between(Mover::Guest, start, exit) {
                let guest = &mut self.guests[index];
                guest.state = GuestState::WalkingToExit;
                guest.route = route;
                guest.route_index = 0;
            }
        }
    }

    fn add_litter(&mut self, position: Position) -> bool {
        if !self.walkable(Mover::Guest, position)
            || self
                .litter
                .iter()
                .any(|task| task.x == position.x && task.y == position.y)
        {
            return false;
        }
        let id = self.next_litter_id;
        self.next_litter_id = self.next_litter_id.saturating_add(1);
        self.litter.push(LitterTask {
            id,
            x: position.x,
            y: position.y,
            created_minute: self.absolute_minute(),
            assigned_janitor_id: None,
        });
        true
    }

    fn oldest_litter_age_minutes(&self) -> u64 {
        let now = self.absolute_minute();
        self.litter
            .iter()
            .map(|task| now.saturating_sub(task.created_minute))
            .max()
            .unwrap_or(0)
    }

    fn park_cleanliness(&self) -> u32 {
        let backlog_penalty = (self.litter.len() as u32).saturating_mul(12);
        let age_penalty = u32::try_from(self.oldest_litter_age_minutes() / 15)
            .unwrap_or(u32::MAX)
            .min(30);
        100_u32.saturating_sub(backlog_penalty.saturating_add(age_penalty).min(90))
    }

    fn release_unreachable_janitor_assignments(&mut self) {
        let assignments: Vec<(usize, u32, Position)> = self
            .janitors
            .iter()
            .enumerate()
            .filter_map(|(index, janitor)| {
                Some((
                    index,
                    janitor.target_litter_id?,
                    Position {
                        x: janitor.x,
                        y: janitor.y,
                    },
                ))
            })
            .collect();

        for (janitor_index, litter_id, janitor_position) in assignments {
            let target = self
                .litter
                .iter()
                .find(|task| task.id == litter_id)
                .map(|task| Position {
                    x: task.x,
                    y: task.y,
                });
            let reachable = target.is_some_and(|target| {
                self.path_between(Mover::Staff, janitor_position, target)
                    .is_some()
            });
            if reachable {
                continue;
            }
            self.note_route_invalidated();
            let janitor_id = self.janitors[janitor_index].id;
            self.janitors[janitor_index].target_litter_id = None;
            if let Some(task) = self.litter.iter_mut().find(|task| task.id == litter_id)
                && task.assigned_janitor_id == Some(janitor_id)
            {
                task.assigned_janitor_id = None;
            }
        }
    }

    fn assign_janitor_tasks(&mut self) {
        for janitor_index in 0..self.janitors.len() {
            if self.janitors[janitor_index].target_litter_id.is_some() {
                continue;
            }
            let janitor_position = Position {
                x: self.janitors[janitor_index].x,
                y: self.janitors[janitor_index].y,
            };
            let target_id = self
                .litter
                .iter()
                .filter(|task| task.assigned_janitor_id.is_none())
                .filter(|task| {
                    self.path_between(
                        Mover::Staff,
                        janitor_position,
                        Position {
                            x: task.x,
                            y: task.y,
                        },
                    )
                    .is_some()
                })
                .min_by_key(|task| task.id)
                .map(|task| task.id);
            let Some(target_id) = target_id else {
                continue;
            };
            let janitor_id = self.janitors[janitor_index].id;
            self.janitors[janitor_index].target_litter_id = Some(target_id);
            if let Some(task) = self.litter.iter_mut().find(|task| task.id == target_id) {
                task.assigned_janitor_id = Some(janitor_id);
            }
        }
    }

    fn complete_litter_task(&mut self, janitor_index: usize, litter_id: u32) {
        self.litter.retain(|task| task.id != litter_id);
        self.janitors[janitor_index].target_litter_id = None;
        self.janitors[janitor_index].tasks_completed = self.janitors[janitor_index]
            .tasks_completed
            .saturating_add(1);
    }

    fn advance_janitor_work(&mut self) {
        self.release_unreachable_janitor_assignments();
        self.assign_janitor_tasks();

        for janitor_index in 0..self.janitors.len() {
            let Some(litter_id) = self.janitors[janitor_index].target_litter_id else {
                continue;
            };
            let Some(target) = self
                .litter
                .iter()
                .find(|task| task.id == litter_id)
                .map(|task| Position {
                    x: task.x,
                    y: task.y,
                })
            else {
                self.janitors[janitor_index].target_litter_id = None;
                continue;
            };
            let current = Position {
                x: self.janitors[janitor_index].x,
                y: self.janitors[janitor_index].y,
            };
            let Some(route) = self.path_between(Mover::Staff, current, target) else {
                continue;
            };
            if route.len() <= 1 {
                self.complete_litter_task(janitor_index, litter_id);
                continue;
            }
            let next = route[1];
            self.janitors[janitor_index].x = next.x;
            self.janitors[janitor_index].y = next.y;
            if next == target {
                self.complete_litter_task(janitor_index, litter_id);
            }
        }

        self.assign_janitor_tasks();
    }

    fn concession_service_state(&self, stand: &Concession) -> &'static str {
        if stand.condition <= MAINTENANCE_FAILURE_THRESHOLD {
            "failed"
        } else if stand.condition <= MAINTENANCE_SERVICE_THRESHOLD {
            "degraded"
        } else {
            "healthy"
        }
    }

    /// The stand's service points (#98): every adjacent path tile a mechanic may work from,
    /// public or staff-only, in neighbour order.
    fn concession_service_tiles(&self, concession_id: u32) -> Vec<Position> {
        self.concession_adjacent(concession_id, Mover::Staff)
    }

    /// The adjacent public tiles guests buy from.
    fn concession_customer_tiles(&self, concession_id: u32) -> Vec<Position> {
        self.concession_adjacent(concession_id, Mover::Guest)
    }

    fn concession_adjacent(&self, concession_id: u32, mover: Mover) -> Vec<Position> {
        let Some(stand) = self
            .concessions
            .iter()
            .find(|stand| stand.id == concession_id)
        else {
            return Vec::new();
        };
        self.neighbors(Position {
            x: stand.x,
            y: stand.y,
        })
        .into_iter()
        .filter(|position| self.walkable(mover, *position))
        .collect()
    }

    fn concession_service_tile_from(
        &self,
        concession_id: u32,
        start: Position,
    ) -> Option<Position> {
        self.concession_service_tiles(concession_id)
            .into_iter()
            .filter_map(|target| {
                self.path_between(Mover::Staff, start, target)
                    .map(|route| (route.len(), target))
            })
            .min_by_key(|(route_len, target)| (*route_len, target.y, target.x))
            .map(|(_, target)| target)
    }

    /// Presentation of a stand's service points; uncounted, so snapshots never add route work.
    fn concession_service_view(&self, stand_id: u32) -> (Vec<ServicePointView>, Option<Position>) {
        let depot_access = self.depot_access_tiles();
        let mut active: Option<(usize, Position)> = None;
        let points = self
            .concession_service_tiles(stand_id)
            .into_iter()
            .map(|point| {
                let route = self.search_path(Mover::Staff, point, &depot_access);
                if let Some(route) = &route {
                    let key = (route.len(), point.y, point.x);
                    if active.is_none_or(|(len, best)| key < (len, best.y, best.x)) {
                        active = Some((route.len(), point));
                    }
                }
                ServicePointView {
                    x: point.x,
                    y: point.y,
                    access: self.path_access(point).map_or("public", PathAccess::key),
                    reachable: route.is_some(),
                }
            })
            .collect();
        (points, active.map(|(_, point)| point))
    }

    fn ensure_maintenance_task(&mut self, concession_id: u32) -> bool {
        let needs_service = self
            .concessions
            .iter()
            .find(|stand| stand.id == concession_id)
            .is_some_and(|stand| stand.condition <= MAINTENANCE_SERVICE_THRESHOLD);
        if !needs_service
            || self
                .maintenance
                .iter()
                .any(|task| task.concession_id == concession_id)
        {
            return false;
        }
        let id = self.next_maintenance_id;
        self.next_maintenance_id = self.next_maintenance_id.saturating_add(1);
        self.maintenance.push(MaintenanceTask {
            id,
            concession_id,
            created_minute: self.absolute_minute(),
            assigned_mechanic_id: None,
        });
        true
    }

    fn advance_concession_maintenance(&mut self) {
        let mut due = Vec::new();
        for stand in &mut self.concessions {
            stand.condition = stand
                .condition
                .saturating_sub(MAINTENANCE_WEAR_PER_INTERVAL);
            if stand.condition <= MAINTENANCE_SERVICE_THRESHOLD {
                due.push(stand.id);
            }
        }
        for concession_id in due {
            self.ensure_maintenance_task(concession_id);
        }
    }

    fn oldest_maintenance_age_minutes(&self) -> u64 {
        let now = self.absolute_minute();
        self.maintenance
            .iter()
            .map(|task| now.saturating_sub(task.created_minute))
            .max()
            .unwrap_or(0)
    }

    fn release_unreachable_mechanic_assignments(&mut self) {
        let assignments: Vec<(usize, u32, Position)> = self
            .mechanics
            .iter()
            .enumerate()
            .filter_map(|(index, mechanic)| {
                Some((
                    index,
                    mechanic.target_maintenance_id?,
                    Position {
                        x: mechanic.x,
                        y: mechanic.y,
                    },
                ))
            })
            .collect();

        for (mechanic_index, task_id, mechanic_position) in assignments {
            let target = self
                .maintenance
                .iter()
                .find(|task| task.id == task_id)
                .and_then(|task| {
                    self.concession_service_tile_from(task.concession_id, mechanic_position)
                });
            let reachable = target.is_some();
            if reachable {
                continue;
            }
            self.note_route_invalidated();
            let mechanic_id = self.mechanics[mechanic_index].id;
            self.mechanics[mechanic_index].target_maintenance_id = None;
            if let Some(task) = self.maintenance.iter_mut().find(|task| task.id == task_id)
                && task.assigned_mechanic_id == Some(mechanic_id)
            {
                task.assigned_mechanic_id = None;
            }
        }
    }

    fn assign_mechanic_tasks(&mut self) {
        for mechanic_index in 0..self.mechanics.len() {
            if self.mechanics[mechanic_index]
                .target_maintenance_id
                .is_some()
            {
                continue;
            }
            let mechanic_position = Position {
                x: self.mechanics[mechanic_index].x,
                y: self.mechanics[mechanic_index].y,
            };
            let target_id = self
                .maintenance
                .iter()
                .filter(|task| task.assigned_mechanic_id.is_none())
                .filter(|task| {
                    self.concession_service_tile_from(task.concession_id, mechanic_position)
                        .is_some()
                })
                .min_by_key(|task| task.id)
                .map(|task| task.id);
            let Some(target_id) = target_id else {
                continue;
            };
            let mechanic_id = self.mechanics[mechanic_index].id;
            self.mechanics[mechanic_index].target_maintenance_id = Some(target_id);
            if let Some(task) = self
                .maintenance
                .iter_mut()
                .find(|task| task.id == target_id)
            {
                task.assigned_mechanic_id = Some(mechanic_id);
            }
        }
    }

    fn complete_maintenance_task(&mut self, mechanic_index: usize, task_id: u32) -> bool {
        let Some(concession_id) = self
            .maintenance
            .iter()
            .find(|task| task.id == task_id)
            .map(|task| task.concession_id)
        else {
            self.mechanics[mechanic_index].target_maintenance_id = None;
            return false;
        };
        let Some(concession_index) = self
            .concessions
            .iter()
            .position(|stand| stand.id == concession_id)
        else {
            self.maintenance.retain(|task| task.id != task_id);
            self.mechanics[mechanic_index].target_maintenance_id = None;
            return false;
        };
        if self
            .spend(
                self.catalog.balance.maintenance_repair_cents,
                ExpenseCategory::MaintenanceRepair,
            )
            .is_err()
        {
            return false;
        }
        self.concessions[concession_index].condition = 100;
        self.maintenance.retain(|task| task.id != task_id);
        self.mechanics[mechanic_index].target_maintenance_id = None;
        self.mechanics[mechanic_index].repairs_completed = self.mechanics[mechanic_index]
            .repairs_completed
            .saturating_add(1);
        true
    }

    fn advance_mechanic_work(&mut self) {
        self.release_unreachable_mechanic_assignments();
        self.assign_mechanic_tasks();

        for mechanic_index in 0..self.mechanics.len() {
            let Some(task_id) = self.mechanics[mechanic_index].target_maintenance_id else {
                continue;
            };
            let current = Position {
                x: self.mechanics[mechanic_index].x,
                y: self.mechanics[mechanic_index].y,
            };
            let Some(concession_id) = self
                .maintenance
                .iter()
                .find(|task| task.id == task_id)
                .map(|task| task.concession_id)
            else {
                self.mechanics[mechanic_index].target_maintenance_id = None;
                continue;
            };
            let Some(target) = self.concession_service_tile_from(concession_id, current) else {
                self.mechanics[mechanic_index].target_maintenance_id = None;
                continue;
            };
            let Some(route) = self.path_between(Mover::Staff, current, target) else {
                continue;
            };
            if route.len() <= 1 {
                self.complete_maintenance_task(mechanic_index, task_id);
                continue;
            }
            let next = route[1];
            self.mechanics[mechanic_index].x = next.x;
            self.mechanics[mechanic_index].y = next.y;
            if next == target {
                self.complete_maintenance_task(mechanic_index, task_id);
            }
        }

        self.assign_mechanic_tasks();
    }

    fn charge_upkeep(&mut self) {
        // Catalog values are bounded per field, not per park, so sums saturate and an
        // unaffordable total is rejected by the cash/ledger range checks below.
        let balance = &self.catalog.balance;
        let times = |count: usize, cents: i64| (count as i64).saturating_mul(cents);
        let habitat_upkeep = self.habitats.iter().fold(0_i64, |total, habitat| {
            let animals = habitat.species.as_ref().map_or(0, |species| {
                times(habitat.animal_count() as usize, species.hourly_upkeep_cents)
            });
            total
                .saturating_add(balance.upkeep.habitat_hourly_cents)
                .saturating_add(animals)
                .saturating_add(times(
                    habitat.fence_length() as usize,
                    habitat.fence.segment_hourly_cents,
                ))
        });
        let park_upkeep = self
            .concessions
            .iter()
            .fold(habitat_upkeep, |total, stand| {
                total.saturating_add(stand.definition.hourly_upkeep_cents)
            });
        let keeper_wages = times(self.keepers.len(), balance.staff.keeper.hourly_wage_cents);
        let janitor_wages = times(self.janitors.len(), balance.staff.janitor.hourly_wage_cents);
        let mechanic_wages = times(
            self.mechanics.len(),
            balance.staff.mechanic.hourly_wage_cents,
        );
        let charges = [
            (ExpenseCategory::ParkUpkeep, park_upkeep),
            (ExpenseCategory::KeeperWages, keeper_wages),
            (ExpenseCategory::JanitorWages, janitor_wages),
            (ExpenseCategory::MechanicWages, mechanic_wages),
        ];
        let mut ledger = self.finance_today;
        let mut cash = self.cash_cents;
        for (category, cents) in charges {
            let Some(next_cash) = cash
                .checked_sub(cents)
                .filter(|cash| (-content::MAX_CENTS..=content::MAX_CENTS).contains(cash))
            else {
                return;
            };
            if !ledger.record_expense(category, cents) {
                return;
            }
            cash = next_cash;
        }
        self.cash_cents = cash;
        self.finance_today = ledger;
    }

    /// Park rating derived from the current state, so every command and tick reports a
    /// self-consistent value without a separate refresh step.
    fn rating(&self) -> u32 {
        let appeal: u32 = self.habitats.iter().map(Habitat::appeal).sum();
        let welfare = if self.habitats.iter().any(|habitat| habitat.stocked()) {
            let total: u32 = self
                .habitats
                .iter()
                .filter(|habitat| habitat.stocked())
                .map(|habitat| habitat.welfare)
                .sum();
            let count = self
                .habitats
                .iter()
                .filter(|habitat| habitat.stocked())
                .count() as u32;
            total / count.max(1)
        } else {
            50
        };
        let guest_happiness = if self.guests.is_empty() {
            60
        } else {
            self.guests.iter().map(|guest| guest.happiness).sum::<u32>() / self.guests.len() as u32
        };
        let cleanliness_penalty = (100_u32.saturating_sub(self.park_cleanliness())) * 2;
        (250 + appeal / 3 + welfare * 2 + guest_happiness)
            .saturating_sub(cleanliness_penalty)
            .clamp(0, 999)
    }

    fn neighbors(&self, position: Position) -> Vec<Position> {
        let mut neighbors = Vec::with_capacity(4);
        if position.x > 0 {
            neighbors.push(Position {
                x: position.x - 1,
                y: position.y,
            });
        }
        if position.x + 1 < self.width {
            neighbors.push(Position {
                x: position.x + 1,
                y: position.y,
            });
        }
        if position.y > 0 {
            neighbors.push(Position {
                x: position.x,
                y: position.y - 1,
            });
        }
        if position.y + 1 < self.height {
            neighbors.push(Position {
                x: position.x,
                y: position.y + 1,
            });
        }
        neighbors
    }

    /// Whether `position` is a path surface anyone walks on (public or staff-only path, or the
    /// entrance). Physical questions use this; routes use [`Self::walkable`].
    fn is_walkable(&self, position: Position) -> bool {
        matches!(
            self.tile(position.x, position.y),
            Some(TileKind::Path(_) | TileKind::Entrance)
        )
    }

    /// The route permission table (#98): guests walk public paths and the entrance, every staff
    /// job also walks staff-only paths.
    fn walkable(&self, mover: Mover, position: Position) -> bool {
        match self.tile(position.x, position.y) {
            Some(TileKind::Entrance | TileKind::Path(PathAccess::Public)) => true,
            Some(TileKind::Path(PathAccess::Staff)) => mover == Mover::Staff,
            _ => false,
        }
    }

    /// The access class of a path tile; the entrance counts as public.
    fn path_access(&self, position: Position) -> Option<PathAccess> {
        match self.tile(position.x, position.y) {
            Some(TileKind::Entrance) => Some(PathAccess::Public),
            Some(TileKind::Path(access)) => Some(access),
            _ => None,
        }
    }

    /// Whether a guest at `viewpoint`, looking over the fence on its `side`, sees `target`:
    /// enclosed tiles up to [`VIEWING_DEPTH_TILES`] past that fence and within
    /// [`VIEWING_HALF_WIDTH_TILES`] to either side (#81), for any enclosure shape.
    fn viewing_tile_visible(
        &self,
        habitat: &Habitat,
        viewpoint: Position,
        side: FenceSide,
        target: Position,
    ) -> bool {
        if !habitat.contains(target) {
            return false;
        }
        let depth = |fence: u32, toward: u32, inward_positive: bool| {
            if inward_positive {
                toward.checked_sub(fence)
            } else {
                fence.checked_sub(toward)
            }
            .is_some_and(|depth| depth < VIEWING_DEPTH_TILES)
        };
        match side {
            FenceSide::North => {
                depth(viewpoint.y + 1, target.y, true)
                    && target.x.abs_diff(viewpoint.x) <= VIEWING_HALF_WIDTH_TILES
            }
            FenceSide::South => {
                viewpoint
                    .y
                    .checked_sub(1)
                    .is_some_and(|fence| depth(fence, target.y, false))
                    && target.x.abs_diff(viewpoint.x) <= VIEWING_HALF_WIDTH_TILES
            }
            FenceSide::West => {
                depth(viewpoint.x + 1, target.x, true)
                    && target.y.abs_diff(viewpoint.y) <= VIEWING_HALF_WIDTH_TILES
            }
            FenceSide::East => {
                viewpoint
                    .x
                    .checked_sub(1)
                    .is_some_and(|fence| depth(fence, target.x, false))
                    && target.y.abs_diff(viewpoint.y) <= VIEWING_HALF_WIDTH_TILES
            }
        }
    }

    #[cfg(test)]
    fn viewing_footprint(
        &self,
        habitat: &Habitat,
        viewpoint: Position,
        side: FenceSide,
    ) -> Vec<Position> {
        habitat
            .tiles
            .iter()
            .copied()
            .filter(|tile| self.viewing_tile_visible(habitat, viewpoint, side, *tile))
            .collect()
    }

    /// Counts the animals whose authoritative tiles are visible from `viewpoint`.
    fn visible_animals_from(&self, habitat: &Habitat, viewpoint: Position, side: FenceSide) -> u32 {
        habitat
            .animals
            .iter()
            .filter(|animal| self.viewing_tile_visible(habitat, viewpoint, side, animal.position))
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    /// The outside tiles along the fence with the side each looks over, north side first, then
    /// east, south and west. A tile facing two fence segments in a notch views over the first.
    fn fence_outside_tiles(&self, habitat: &Habitat) -> Vec<(Position, FenceSide)> {
        let mut candidates: Vec<(Position, FenceSide)> = habitat
            .fence_segments()
            .into_iter()
            .filter_map(|segment| {
                let outside = self.gate_access_tile(segment)?;
                Some((outside, segment.side))
            })
            .collect();
        let rank = |side: FenceSide| match side {
            FenceSide::North => 0,
            FenceSide::East => 1,
            FenceSide::South => 2,
            FenceSide::West => 3,
        };
        candidates.sort_by_key(|(position, side)| match side {
            FenceSide::North | FenceSide::South => (rank(*side), position.y, position.x),
            FenceSide::East | FenceSide::West => (rank(*side), position.x, position.y),
        });
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|(position, _)| seen.insert((position.x, position.y)));
        candidates
    }

    fn viewing_side_for_position(
        &self,
        habitat: &Habitat,
        position: Position,
    ) -> Option<FenceSide> {
        self.fence_outside_tiles(habitat)
            .into_iter()
            .find(|(outside, _)| *outside == position)
            .map(|(_, side)| side)
    }

    fn viewing_spot(
        &self,
        habitat: &Habitat,
        position: Position,
        side: FenceSide,
        occupancy: u32,
    ) -> ViewingSpot {
        let visible_animals = self.visible_animals_from(habitat, position, side);
        ViewingSpot {
            position,
            side,
            visible_animals,
            capacity: visible_animals
                .saturating_mul(VIEWERS_PER_VISIBLE_ANIMAL)
                .min(VIEWPOINT_PHYSICAL_CAPACITY),
            occupancy,
        }
    }

    fn viewing_occupancy(&self) -> ViewingOccupancy {
        let mut occupancy = ViewingOccupancy::new();
        for guest in self
            .guests
            .iter()
            .filter(|guest| guest.state == GuestState::Viewing)
        {
            let entry = occupancy.entry((guest.x, guest.y)).or_default();
            *entry = entry.saturating_add(1);
        }
        occupancy
    }

    fn viewing_spots_with_occupancy(
        &self,
        habitat: &Habitat,
        viewing_occupancy: &ViewingOccupancy,
    ) -> Vec<ViewingSpot> {
        self.fence_outside_tiles(habitat)
            .into_iter()
            .filter(|(position, _)| self.walkable(Mover::Guest, *position))
            .map(|(position, side)| {
                let occupancy = viewing_occupancy
                    .get(&(position.x, position.y))
                    .copied()
                    .unwrap_or(0);
                self.viewing_spot(habitat, position, side, occupancy)
            })
            .collect()
    }

    fn viewing_spot_for_guest_with_occupancy(
        &self,
        guest: &Guest,
        viewing_occupancy: &ViewingOccupancy,
    ) -> Option<ViewingSpot> {
        let habitat = self
            .habitats
            .iter()
            .find(|habitat| habitat.id == guest.target_habitat)?;
        let position = Position {
            x: guest.x,
            y: guest.y,
        };
        if !self.walkable(Mover::Guest, position) {
            return None;
        }
        let side = self.viewing_side_for_position(habitat, position)?;
        let occupancy = viewing_occupancy
            .get(&(position.x, position.y))
            .copied()
            .unwrap_or(0);
        Some(self.viewing_spot(habitat, position, side, occupancy))
    }

    #[cfg(test)]
    fn viewing_spot_for_guest(&self, guest: &Guest) -> Option<ViewingSpot> {
        let viewing_occupancy = self.viewing_occupancy();
        self.viewing_spot_for_guest_with_occupancy(guest, &viewing_occupancy)
    }

    fn guest_thought_with_occupancy(
        &self,
        guest: &Guest,
        viewing_occupancy: &ViewingOccupancy,
    ) -> &'static str {
        if guest.route.is_empty()
            && matches!(
                guest.state,
                GuestState::Arriving | GuestState::WalkingToHabitat
            )
        {
            return "The path to the animals is blocked.";
        }
        if (guest.route.is_empty() && guest.state == GuestState::WalkingToExit)
            || (guest.state == GuestState::Viewing && guest.viewing_minutes == 0)
        {
            return "The path to the exit is blocked.";
        }
        if guest.thirst >= 60 {
            return "I'm getting thirsty.";
        }
        if guest.hunger >= 60 {
            return "I could use something to eat.";
        }
        if guest.energy <= 35 {
            return "My feet are getting tired.";
        }
        if guest.state == GuestState::Viewing
            && let Some(spot) = self.viewing_spot_for_guest_with_occupancy(guest, viewing_occupancy)
        {
            if spot.visible_animals == 0 {
                return "I can't see any animals from here.";
            }
            if spot.crowded() {
                return "It's too crowded to get a good view.";
            }
        }
        if guest.cleanliness_concern {
            return "The paths need cleaning.";
        }
        if guest.value_perception <= 40 {
            return "I expected a little more for the price.";
        }

        match guest.state {
            GuestState::Arriving => "I'm entering the zoo.",
            GuestState::WalkingToHabitat if !guest.visited_habitats.is_empty() => {
                "I'd like to see another habitat."
            }
            GuestState::WalkingToHabitat => "I want to see the animals.",
            GuestState::Viewing if guest.visited_habitats.len() > 1 => {
                "There is a lot to see here."
            }
            GuestState::Viewing => "The animals are wonderful.",
            GuestState::WalkingToExit => "I'm ready to head home.",
        }
    }

    #[cfg(test)]
    fn guest_thought(&self, guest: &Guest) -> &'static str {
        let viewing_occupancy = self.viewing_occupancy();
        self.guest_thought_with_occupancy(guest, &viewing_occupancy)
    }

    fn viewing_route_with_occupancy(
        &self,
        habitat: &Habitat,
        start: Position,
        viewing_occupancy: &ViewingOccupancy,
    ) -> Option<Vec<Position>> {
        let mut candidates: Vec<(ViewingSpot, Vec<Position>)> = self
            .viewing_spots_with_occupancy(habitat, viewing_occupancy)
            .into_iter()
            .filter(|spot| spot.capacity > 0)
            .filter_map(|spot| {
                self.path_between(Mover::Guest, start, spot.position)
                    .map(|route| (spot, route))
            })
            .collect();

        candidates.sort_by(|(left_spot, left_route), (right_spot, right_route)| {
            let left_load =
                u64::from(left_spot.occupancy).saturating_mul(u64::from(right_spot.capacity));
            let right_load =
                u64::from(right_spot.occupancy).saturating_mul(u64::from(left_spot.capacity));
            right_spot
                .available_capacity()
                .cmp(&left_spot.available_capacity())
                .then_with(|| left_load.cmp(&right_load))
                .then_with(|| right_spot.visible_animals.cmp(&left_spot.visible_animals))
                .then_with(|| left_route.len().cmp(&right_route.len()))
                .then_with(|| left_spot.position.y.cmp(&right_spot.position.y))
                .then_with(|| left_spot.position.x.cmp(&right_spot.position.x))
        });

        candidates.into_iter().next().map(|(_, route)| route)
    }

    #[cfg(test)]
    fn viewing_route(&self, habitat: &Habitat, start: Position) -> Option<Vec<Position>> {
        let viewing_occupancy = self.viewing_occupancy();
        self.viewing_route_with_occupancy(habitat, start, &viewing_occupancy)
    }

    fn path_between(&self, mover: Mover, start: Position, goal: Position) -> Option<Vec<Position>> {
        if !self.walkable(mover, goal) {
            return None;
        }
        self.path_to_any(mover, start, &[goal])
    }

    fn path_to_any(
        &self,
        mover: Mover,
        start: Position,
        goals: &[Position],
    ) -> Option<Vec<Position>> {
        if !self.walkable(mover, start) || goals.is_empty() {
            return None;
        }
        self.count_path_search();
        self.search_path(mover, start, goals)
    }

    fn count_path_search(&self) {
        let searches = &self.work.path_searches;
        searches.set(searches.get().saturating_add(1));
    }

    /// The breadth-first walk shared by every staff and guest route. Presentation queries call
    /// it directly so rendering a snapshot never changes the simulation work counters.
    fn search_path(
        &self,
        mover: Mover,
        start: Position,
        goals: &[Position],
    ) -> Option<Vec<Position>> {
        if !self.walkable(mover, start) || goals.is_empty() {
            return None;
        }

        let mut queue = VecDeque::from([start]);
        let mut previous: HashMap<(u32, u32), Option<Position>> =
            HashMap::from([((start.x, start.y), None)]);

        // One BFS considers all access points. Equal-length routes follow the
        // existing neighbor order; randomized HashMap iteration is never used.
        while let Some(current) = queue.pop_front() {
            if goals.contains(&current) {
                let mut route = vec![current];
                let mut cursor = current;
                while let Some(Some(parent)) = previous.get(&(cursor.x, cursor.y)) {
                    route.push(*parent);
                    cursor = *parent;
                }
                route.reverse();
                return Some(route);
            }

            for next in self.neighbors(current) {
                if !self.walkable(mover, next) || previous.contains_key(&(next.x, next.y)) {
                    continue;
                }
                previous.insert((next.x, next.y), Some(current));
                queue.push_back(next);
            }
        }
        None
    }

    fn complaint_summary(&self) -> ComplaintSummary {
        ComplaintSummary {
            hungry: self
                .guests
                .iter()
                .filter(|guest| guest.hunger >= 60)
                .count() as u32,
            thirsty: self
                .guests
                .iter()
                .filter(|guest| guest.thirst >= 60)
                .count() as u32,
            tired: self
                .guests
                .iter()
                .filter(|guest| guest.energy <= 35)
                .count() as u32,
            poor_value: self
                .guests
                .iter()
                .filter(|guest| guest.value_perception <= 40)
                .count() as u32,
        }
    }

    fn species_catalog(&self) -> Vec<SpeciesOfferView> {
        self.catalog
            .species
            .iter()
            .filter(|species| self.unlocks.species_unlocked(&species.key))
            .map(|species| {
                let profile = species.welfare_profile();
                SpeciesOfferView {
                    key: species.key().to_owned(),
                    asset_ref: species.asset_ref.clone(),
                    label: species.label().to_owned(),
                    purchase_cost_cents: species.purchase_cost(),
                    appeal: species.appeal(),
                    minimum_social_group: profile.minimum_social_group,
                    space_per_animal: profile.space_per_animal,
                    minimum_fence_height_cm: species.minimum_fence_height_cm,
                    habitat: species.habitat.clone(),
                }
            })
            .collect()
    }

    fn animal_views(&self) -> Vec<AnimalView> {
        self.habitats
            .iter()
            .flat_map(|habitat| {
                habitat.animals.iter().map(move |animal| AnimalView {
                    id: animal.id,
                    habitat_id: habitat.id,
                    species: animal.species.clone(),
                    name: animal.name.clone(),
                    sex: animal.sex,
                    life_stage: animal.life_stage,
                    arrived_day: animal.arrived_day,
                    x: animal.position.x,
                    y: animal.position.y,
                    activity: animal.activity_label(),
                    target: match animal.activity {
                        animal::Activity::Walking { target } => Some(target),
                        animal::Activity::Resting { .. } => None,
                    },
                    energy: animal.energy,
                    cosmetic_seed: animal.cosmetic_seed,
                    animation_phase: animal.cosmetic_seed % 100,
                })
            })
            .collect()
    }

    fn feeding_status(&self, habitat: &Habitat) -> String {
        if let Some(problem) = self.staffing_problem(habitat) {
            return problem;
        }
        if self.feed_crates == 0 {
            return "Feeding waits for depot feed stock".to_owned();
        }
        let next_feeding = habitat.care_schedule.as_ref().and_then(|schedule| {
            schedule
                .tasks
                .iter()
                .find(|task| task.category == content::CareCategory::Feeding)
        });
        match next_feeding {
            Some(task) => format!(
                "Next feeding in {} min",
                task.next_due_minute.saturating_sub(self.absolute_minute())
            ),
            None => "Care starts with the first animals".to_owned(),
        }
    }

    fn janitor_status(&self, janitor: &Janitor) -> String {
        if let Some(litter_id) = janitor.target_litter_id
            && let Some(task) = self.litter.iter().find(|task| task.id == litter_id)
        {
            let age = self.absolute_minute().saturating_sub(task.created_minute);
            return format!("Cleaning litter #{litter_id} · {age} min old");
        }
        if self.litter.is_empty() {
            return "Idle · park paths are clean".to_owned();
        }
        let position = Position {
            x: janitor.x,
            y: janitor.y,
        };
        let reachable_unassigned = self.litter.iter().any(|task| {
            task.assigned_janitor_id.is_none()
                && self
                    .path_between(
                        Mover::Staff,
                        position,
                        Position {
                            x: task.x,
                            y: task.y,
                        },
                    )
                    .is_some()
        });
        if reachable_unassigned {
            return "Available for cleanup".to_owned();
        }
        let reachable = self.litter.iter().any(|task| {
            self.path_between(
                Mover::Staff,
                position,
                Position {
                    x: task.x,
                    y: task.y,
                },
            )
            .is_some()
        });
        if reachable {
            "Idle · cleanup work is already assigned".to_owned()
        } else {
            "Blocked · no reachable litter task".to_owned()
        }
    }

    fn litter_status(&self, task: &LitterTask) -> String {
        let target = Position {
            x: task.x,
            y: task.y,
        };
        if let Some(janitor_id) = task.assigned_janitor_id
            && let Some(janitor) = self
                .janitors
                .iter()
                .find(|janitor| janitor.id == janitor_id)
        {
            let reachable = self
                .path_between(
                    Mover::Staff,
                    Position {
                        x: janitor.x,
                        y: janitor.y,
                    },
                    target,
                )
                .is_some();
            return if reachable {
                format!("Janitor #{janitor_id} responding")
            } else {
                format!("Blocked · Janitor #{janitor_id} route disconnected")
            };
        }
        if self.janitors.is_empty() {
            return "Waiting · no janitor hired".to_owned();
        }
        if self.janitors.iter().any(|janitor| {
            self.path_between(
                Mover::Staff,
                Position {
                    x: janitor.x,
                    y: janitor.y,
                },
                target,
            )
            .is_some()
        }) {
            "Waiting for an available janitor".to_owned()
        } else {
            "Blocked · disconnected from janitors".to_owned()
        }
    }

    fn mechanic_status(&self, mechanic: &Mechanic) -> String {
        if let Some(task_id) = mechanic.target_maintenance_id
            && let Some(task) = self.maintenance.iter().find(|task| task.id == task_id)
        {
            let age = self.absolute_minute().saturating_sub(task.created_minute);
            let position = Position {
                x: mechanic.x,
                y: mechanic.y,
            };
            let at_service_tile = self
                .concession_service_tiles(task.concession_id)
                .contains(&position);
            if at_service_tile && self.cash_cents < self.catalog.balance.maintenance_repair_cents {
                return format!(
                    "Waiting for repair parts at stand #{} · ${} needed",
                    task.concession_id,
                    self.catalog.balance.maintenance_repair_cents / 100
                );
            }
            return format!(
                "Responding to stand #{} · {age} min old",
                task.concession_id
            );
        }
        if self.maintenance.is_empty() {
            return "Idle · facilities are maintained".to_owned();
        }
        let position = Position {
            x: mechanic.x,
            y: mechanic.y,
        };
        let reachable = self.maintenance.iter().any(|task| {
            self.concession_service_tile_from(task.concession_id, position)
                .is_some()
        });
        if reachable {
            "Available for maintenance".to_owned()
        } else {
            "Blocked · no reachable maintenance task".to_owned()
        }
    }

    fn maintenance_status(&self, task: &MaintenanceTask) -> String {
        if self.concession_service_tiles(task.concession_id).is_empty() {
            return "Blocked · stand has no path service point".to_owned();
        }
        if let Some(mechanic_id) = task.assigned_mechanic_id
            && let Some(mechanic) = self
                .mechanics
                .iter()
                .find(|mechanic| mechanic.id == mechanic_id)
        {
            let position = Position {
                x: mechanic.x,
                y: mechanic.y,
            };
            let reachable = self
                .concession_service_tile_from(task.concession_id, position)
                .is_some();
            return if reachable {
                format!("Mechanic #{mechanic_id} responding")
            } else {
                format!("Blocked · Mechanic #{mechanic_id} route disconnected")
            };
        }
        if self.mechanics.is_empty() {
            return "Waiting · no mechanic hired".to_owned();
        }
        if self.mechanics.iter().any(|mechanic| {
            self.concession_service_tile_from(
                task.concession_id,
                Position {
                    x: mechanic.x,
                    y: mechanic.y,
                },
            )
            .is_some()
        }) {
            "Waiting for an available mechanic".to_owned()
        } else {
            "Blocked · disconnected from mechanics".to_owned()
        }
    }

    fn concession_maintenance_status(&self, stand: &Concession) -> String {
        self.maintenance
            .iter()
            .find(|task| task.concession_id == stand.id)
            .map_or_else(
                || "No maintenance due".to_owned(),
                |task| self.maintenance_status(task),
            )
    }

    fn animal_care_depot_view(&self) -> AnimalCareDepotView {
        AnimalCareDepotView {
            x: self.depot.x,
            y: self.depot.y,
            feed_crates: self.feed_crates,
            feed_batch_crates: FEED_BATCH_CRATES,
            feed_batch_cost_cents: self.catalog.balance.feed_batch_cents,
            keeper_hire_cost_cents: self.catalog.balance.staff.keeper.hire_cents,
            keeper_hourly_wage_cents: self.catalog.balance.staff.keeper.hourly_wage_cents,
            janitor_hire_cost_cents: self.catalog.balance.staff.janitor.hire_cents,
            janitor_hourly_wage_cents: self.catalog.balance.staff.janitor.hourly_wage_cents,
            mechanic_hire_cost_cents: self.catalog.balance.staff.mechanic.hire_cents,
            mechanic_hourly_wage_cents: self.catalog.balance.staff.mechanic.hourly_wage_cents,
            maintenance_repair_cost_cents: self.catalog.balance.maintenance_repair_cents,
            keepers: self
                .keepers
                .iter()
                .map(|keeper| self.keeper_view(keeper))
                .collect(),
            janitors: self
                .janitors
                .iter()
                .map(|janitor| JanitorView {
                    id: janitor.id,
                    x: janitor.x,
                    y: janitor.y,
                    target_litter_id: janitor.target_litter_id,
                    tasks_completed: janitor.tasks_completed,
                    status: self.janitor_status(janitor),
                })
                .collect(),
            mechanics: self
                .mechanics
                .iter()
                .map(|mechanic| MechanicView {
                    id: mechanic.id,
                    x: mechanic.x,
                    y: mechanic.y,
                    target_maintenance_id: mechanic.target_maintenance_id,
                    repairs_completed: mechanic.repairs_completed,
                    status: self.mechanic_status(mechanic),
                })
                .collect(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        let viewing_occupancy = self.viewing_occupancy();
        let mut tiles = Vec::with_capacity(self.tiles.len());
        for y in 0..self.height {
            for x in 0..self.width {
                let kind = match self.tile(x, y).unwrap_or(TileKind::Grass) {
                    TileKind::Grass => "grass",
                    TileKind::Path(_) => "path",
                    TileKind::Entrance => "entrance",
                    TileKind::Habitat(_) => "habitat",
                    TileKind::Concession(_) => "concession",
                    TileKind::AnimalCareDepot => "grass",
                    TileKind::Water => "water",
                };
                let habitat_id = match self.tile(x, y) {
                    Some(TileKind::Habitat(id)) => Some(id),
                    _ => None,
                };
                let concession_id = match self.tile(x, y) {
                    Some(TileKind::Concession(id)) => Some(id),
                    _ => None,
                };
                tiles.push(TileView {
                    x,
                    y,
                    kind: kind.to_owned(),
                    habitat_id,
                    concession_id,
                    access: match self.tile(x, y) {
                        Some(TileKind::Path(access)) => Some(access.key()),
                        _ => None,
                    },
                });
            }
        }

        let habitats = self
            .habitats
            .iter()
            .map(|habitat| {
                let viewing_spots = self.viewing_spots_with_occupancy(habitat, &viewing_occupancy);
                let viewing_capacity = viewing_spots
                    .iter()
                    .map(|spot| spot.capacity)
                    .fold(0_u32, u32::saturating_add);
                let viewing_occupancy = viewing_spots
                    .iter()
                    .map(|spot| spot.occupancy)
                    .fold(0_u32, u32::saturating_add);
                let gate_reachable = self.service_gate_reachable(habitat);
                let staffing = self.staffing_problem(habitat);
                let work_zone_id = self.habitat_zone(habitat);
                let bounds = shape::bounds(&habitat.tiles);
                HabitatView {
                    id: habitat.id,
                    x: bounds.0,
                    y: bounds.1,
                    width: bounds.2,
                    height: bounds.3,
                    tiles: habitat.tiles.clone(),
                    fence: habitat.fence.clone(),
                    footprint_area: habitat.area(),
                    fence_length: habitat.fence_length(),
                    fence_segments: habitat.fence_segments(),
                    service_gate: ServiceGateView {
                        x: habitat.service_gate.x,
                        y: habitat.service_gate.y,
                        side: habitat.service_gate.side,
                        access: self.gate_access_tile(habitat.service_gate),
                        reachable: gate_reachable,
                    },
                    species: habitat
                        .species
                        .as_ref()
                        .map(|species| species.key().to_owned()),
                    animals: habitat.animal_count(),
                    capacity: habitat.capacity(),
                    viewing_capacity,
                    viewing_occupancy,
                    viewing_spots: viewing_spots
                        .into_iter()
                        .map(|spot| ViewingSpotView {
                            x: spot.position.x,
                            y: spot.position.y,
                            side: spot.side,
                            visible_animals: spot.visible_animals,
                            capacity: spot.capacity,
                            occupancy: spot.occupancy,
                            crowded: spot.crowded(),
                        })
                        .collect(),
                    welfare: habitat.welfare,
                    welfare_target: habitat.welfare_target(),
                    social_score: habitat.social_score(),
                    space_score: habitat.space_score(),
                    welfare_status: habitat.welfare_status(staffing.as_deref(), gate_reachable),
                    food: habitat.food,
                    water: habitat.water,
                    cleanliness: habitat.cleanliness,
                    environment: habitat.environment_view(),
                    welfare_parts: habitat.welfare_parts(),
                    requirements: habitat
                        .species
                        .as_ref()
                        .map(|species| species.habitat.clone()),
                    work_zone_id,
                    zone_keeper_count: work_zone_id
                        .map_or(0, |zone_id| self.zone_keeper_count(zone_id) as u32),
                    care_required_keepers: habitat
                        .care_schedule
                        .as_ref()
                        .map(|schedule| schedule.required_keepers),
                    care_schedule: habitat
                        .care_schedule
                        .as_ref()
                        .map(|schedule| self.care_task_views(schedule))
                        .unwrap_or_default(),
                    care_jobs: self.care_job_views(habitat.id),
                    feeding_status: self.feeding_status(habitat),
                    care_status: habitat.care_status(staffing.as_deref(), gate_reachable),
                    appeal: habitat.appeal(),
                }
            })
            .collect();

        let concessions = self
            .concessions
            .iter()
            .map(|stand| {
                let (service_points, active_service_point) = self.concession_service_view(stand.id);
                (stand, service_points, active_service_point)
            })
            .map(
                |(stand, service_points, active_service_point)| ConcessionView {
                    customer_tiles: self.concession_customer_tiles(stand.id),
                    service_points,
                    active_service_point,
                    id: stand.id,
                    x: stand.x,
                    y: stand.y,
                    kind: stand.definition.key.clone(),
                    build_cost_cents: stand.definition.build_cost_cents,
                    price_cents: stand.definition.price_cents,
                    sales_today: stand.sales_today,
                    total_sales: stand.total_sales,
                    total_revenue_cents: stand.total_revenue_cents,
                    condition: stand.condition,
                    service_state: self.concession_service_state(stand).to_owned(),
                    maintenance_status: self.concession_maintenance_status(stand),
                },
            )
            .collect();

        let guests = self
            .guests
            .iter()
            .map(|guest| GuestView {
                id: guest.id,
                x: guest.x,
                y: guest.y,
                happiness: guest.happiness,
                energy: guest.energy,
                hunger: guest.hunger,
                thirst: guest.thirst,
                value_perception: guest.value_perception,
                target_habitat: guest.target_habitat,
                habitats_viewed: guest.visited_habitats.len() as u32,
                state: guest.state,
                thought: self
                    .guest_thought_with_occupancy(guest, &viewing_occupancy)
                    .to_owned(),
            })
            .collect();

        Snapshot {
            ruleset_id: self.catalog.ruleset_id.clone(),
            facility_catalog: self
                .catalog
                .facilities
                .iter()
                // Landmarks (entrance, depot) are not buildable, so unlocks never hide them.
                .filter(|facility| {
                    ConcessionKind::parse(&facility.service).is_none()
                        || self.unlocks.facility_unlocked(&facility.key)
                })
                .cloned()
                .collect(),
            width: self.width,
            height: self.height,
            day: self.day,
            minute_of_day: self.minute_of_day,
            cash_cents: self.cash_cents,
            rating: self.rating(),
            guest_count: self.guests.len() as u32,
            entrance: EntranceView {
                x: self.entrance.x,
                y: self.entrance.y,
                arrivals_total: self.next_guest_id.saturating_sub(1),
            },
            tiles,
            habitats,
            concessions,
            animal_care_depot: self.animal_care_depot_view(),
            animals: self.animal_views(),
            guests,
            litter: self
                .litter
                .iter()
                .map(|task| LitterView {
                    id: task.id,
                    x: task.x,
                    y: task.y,
                    age_minutes: self.absolute_minute().saturating_sub(task.created_minute),
                    assigned_janitor_id: task.assigned_janitor_id,
                    status: self.litter_status(task),
                })
                .collect(),
            maintenance: self
                .maintenance
                .iter()
                .filter_map(|task| {
                    let stand = self
                        .concessions
                        .iter()
                        .find(|stand| stand.id == task.concession_id)?;
                    Some(MaintenanceView {
                        id: task.id,
                        concession_id: task.concession_id,
                        x: stand.x,
                        y: stand.y,
                        age_minutes: self.absolute_minute().saturating_sub(task.created_minute),
                        assigned_mechanic_id: task.assigned_mechanic_id,
                        status: self.maintenance_status(task),
                    })
                })
                .collect(),
            work_zones: self.work_zone_views(),
            operations: OperationsView {
                cleanliness: self.park_cleanliness(),
                litter_backlog: self.litter.len() as u32,
                oldest_litter_age_minutes: self.oldest_litter_age_minutes(),
                maintenance_backlog: self.maintenance.len() as u32,
                oldest_maintenance_age_minutes: self.oldest_maintenance_age_minutes(),
                degraded_concessions: self
                    .concessions
                    .iter()
                    .filter(|stand| self.concession_service_state(stand) == "degraded")
                    .count() as u32,
                failed_concessions: self
                    .concessions
                    .iter()
                    .filter(|stand| self.concession_service_state(stand) == "failed")
                    .count() as u32,
            },
            species_catalog: self.species_catalog(),
            fence_catalog: self.catalog.fences.clone(),
            habitat_environment: environment::EnvironmentCatalogView::new(&self.catalog),
            complaints: self.complaint_summary(),
            construction: self.construction_view(),
            finance: FinanceView::new(
                self.catalog.balance.admission_price_cents,
                self.day,
                self.finance_today,
                self.finance_previous,
            ),
            tutorial: self.tutorial_view(),
            scenario: self.scenario_view(),
            alerts: self.alerts.clone(),
            summary: self.daily_summary(),
        }
    }
}

#[derive(Serialize)]
struct TileView {
    x: u32,
    y: u32,
    kind: String,
    habitat_id: Option<u32>,
    concession_id: Option<u32>,
    /// `"public"` or `"staff"` on path tiles (#98).
    access: Option<&'static str>,
}

#[derive(Serialize)]
struct ConcessionView {
    id: u32,
    x: u32,
    y: u32,
    kind: String,
    build_cost_cents: i64,
    price_cents: i64,
    sales_today: u32,
    total_sales: u32,
    total_revenue_cents: i64,
    condition: u32,
    service_state: String,
    maintenance_status: String,
    /// The adjacent public tiles guests buy from (#98).
    customer_tiles: Vec<Position>,
    /// Every adjacent path tile a mechanic may work from, with its access class.
    service_points: Vec<ServicePointView>,
    /// The service point a mechanic leaving the depot actually uses: the nearest reachable
    /// one by staff route, ties broken by row then column. `None` when none is reachable.
    active_service_point: Option<Position>,
}

#[derive(Serialize)]
struct ServicePointView {
    x: u32,
    y: u32,
    access: &'static str,
    /// Whether staff can walk there from the depot.
    reachable: bool,
}

#[derive(Serialize)]
struct KeeperView {
    id: u32,
    x: u32,
    y: u32,
    zone_id: Option<u32>,
    /// No work zone: the keeper stays idle until assigned one.
    needs_zone: bool,
    /// Care categories the keeper accepts, in catalog order.
    categories: Vec<content::CareCategory>,
    /// The daily availability window; `shift_end_minute` may wrap past midnight.
    shift_start_minute: u32,
    shift_end_minute: u32,
    on_shift: bool,
    deliveries_completed: u32,
    activity: &'static str,
    job_id: Option<u32>,
    /// The habitat of the keeper's current job.
    habitat_id: Option<u32>,
    /// Remaining walk from the keeper's tile to its destination; empty when blocked.
    route: Vec<Position>,
    status: String,
}

#[derive(Serialize)]
struct ServiceGateView {
    x: u32,
    y: u32,
    side: FenceSide,
    /// The outside tile keepers stand on before entering; never buildable.
    access: Option<Position>,
    reachable: bool,
}

#[derive(Serialize)]
struct JanitorView {
    id: u32,
    x: u32,
    y: u32,
    target_litter_id: Option<u32>,
    tasks_completed: u32,
    status: String,
}

#[derive(Serialize)]
struct MechanicView {
    id: u32,
    x: u32,
    y: u32,
    target_maintenance_id: Option<u32>,
    repairs_completed: u32,
    status: String,
}

#[derive(Serialize)]
struct AnimalCareDepotView {
    x: u32,
    y: u32,
    feed_crates: u32,
    feed_batch_crates: u32,
    feed_batch_cost_cents: i64,
    keeper_hire_cost_cents: i64,
    keeper_hourly_wage_cents: i64,
    janitor_hire_cost_cents: i64,
    janitor_hourly_wage_cents: i64,
    mechanic_hire_cost_cents: i64,
    mechanic_hourly_wage_cents: i64,
    maintenance_repair_cost_cents: i64,
    keepers: Vec<KeeperView>,
    janitors: Vec<JanitorView>,
    mechanics: Vec<MechanicView>,
}

#[derive(Serialize)]
struct LitterView {
    id: u32,
    x: u32,
    y: u32,
    age_minutes: u64,
    assigned_janitor_id: Option<u32>,
    status: String,
}

#[derive(Serialize)]
struct MaintenanceView {
    id: u32,
    concession_id: u32,
    x: u32,
    y: u32,
    age_minutes: u64,
    assigned_mechanic_id: Option<u32>,
    status: String,
}

#[derive(Serialize)]
struct OperationsView {
    cleanliness: u32,
    litter_backlog: u32,
    oldest_litter_age_minutes: u64,
    maintenance_backlog: u32,
    oldest_maintenance_age_minutes: u64,
    degraded_concessions: u32,
    failed_concessions: u32,
}

#[derive(Serialize)]
struct HabitatView {
    id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    /// The enclosed tiles, row-major; x, y, width and height are their bounding box.
    tiles: Vec<Position>,
    fence: content::FenceType,
    footprint_area: u32,
    fence_length: u32,
    fence_segments: Vec<FenceSegment>,
    service_gate: ServiceGateView,
    species: Option<String>,
    animals: u32,
    capacity: u32,
    viewing_capacity: u32,
    viewing_occupancy: u32,
    viewing_spots: Vec<ViewingSpotView>,
    welfare: u32,
    welfare_target: u32,
    social_score: u32,
    space_score: u32,
    welfare_status: String,
    food: u32,
    water: u32,
    cleanliness: u32,
    /// Terrain cells, objects and what the layout offers (#97).
    environment: environment::EnvironmentView,
    /// Every weighted part of the welfare target, explained.
    welfare_parts: Vec<environment::Contribution>,
    /// The housed species' layout requirements.
    requirements: Option<content::HabitatRequirements>,
    /// The work zone containing the service gate; `None` means no keeper can serve it.
    work_zone_id: Option<u32>,
    /// Keepers working in that zone; habitat jobs go only to them.
    zone_keeper_count: u32,
    care_required_keepers: Option<u32>,
    care_schedule: Vec<care::CareTaskView>,
    care_jobs: Vec<care::CareJobView>,
    feeding_status: String,
    care_status: String,
    appeal: u32,
}

#[derive(Serialize)]
struct ViewingSpotView {
    x: u32,
    y: u32,
    side: FenceSide,
    visible_animals: u32,
    capacity: u32,
    occupancy: u32,
    crowded: bool,
}

#[derive(Serialize)]
struct AnimalView {
    id: u32,
    habitat_id: u32,
    species: String,
    name: String,
    sex: animal::Sex,
    life_stage: animal::LifeStage,
    arrived_day: u32,
    x: u32,
    y: u32,
    /// The authoritative current activity, decided by zoo-core.
    activity: &'static str,
    /// Where a walking animal is heading.
    target: Option<Position>,
    energy: u32,
    /// Presentation-only variation: tile offset and animation phase.
    cosmetic_seed: u32,
    animation_phase: u32,
}

#[derive(Serialize)]
struct GuestView {
    id: u32,
    x: u32,
    y: u32,
    happiness: u32,
    energy: u32,
    hunger: u32,
    thirst: u32,
    value_perception: u32,
    target_habitat: u32,
    habitats_viewed: u32,
    state: GuestState,
    thought: String,
}

#[derive(Serialize)]
struct EntranceView {
    x: u32,
    y: u32,
    arrivals_total: u32,
}

#[derive(Serialize)]
struct SpeciesOfferView {
    key: String,
    asset_ref: String,
    label: String,
    purchase_cost_cents: i64,
    appeal: u32,
    minimum_social_group: u32,
    space_per_animal: u32,
    minimum_fence_height_cm: u32,
    /// What a habitat must offer this species (#97).
    habitat: content::HabitatRequirements,
}

#[derive(Serialize)]
struct ComplaintSummary {
    hungry: u32,
    thirsty: u32,
    tired: u32,
    poor_value: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct FinanceDayView {
    day: u32,
    income_cents: i64,
    expenses_cents: i64,
    profit_cents: i64,
    breakdown: FinanceLedger,
}

impl FinanceDayView {
    fn new(day: u32, ledger: FinanceLedger) -> Self {
        Self {
            day,
            income_cents: ledger.income_total_cents(),
            expenses_cents: ledger.expense_total_cents(),
            profit_cents: ledger.profit_cents(),
            breakdown: ledger,
        }
    }
}

#[derive(Serialize)]
struct FinanceView {
    admission_price_cents: i64,
    current_day: FinanceDayView,
    previous_day: Option<FinanceDayView>,
    profit_change_cents: Option<i64>,
    profit_trend: &'static str,
}

impl FinanceView {
    fn new(
        admission_price_cents: i64,
        day: u32,
        current: FinanceLedger,
        previous: Option<FinanceLedger>,
    ) -> Self {
        let current_day = FinanceDayView::new(day, current);
        let previous_day =
            previous.map(|ledger| FinanceDayView::new(day.saturating_sub(1), ledger));
        let profit_change_cents =
            previous_day.map(|previous| current_day.profit_cents - previous.profit_cents);
        let profit_trend = match profit_change_cents {
            None => "no_previous_day",
            Some(change) if change > 0 => "up",
            Some(change) if change < 0 => "down",
            Some(_) => "flat",
        };
        Self {
            admission_price_cents,
            current_day,
            previous_day,
            profit_change_cents,
            profit_trend,
        }
    }
}

#[derive(Serialize)]
struct Snapshot {
    ruleset_id: String,
    facility_catalog: Vec<Facility>,
    width: u32,
    height: u32,
    day: u32,
    minute_of_day: u32,
    cash_cents: i64,
    rating: u32,
    guest_count: u32,
    entrance: EntranceView,
    tiles: Vec<TileView>,
    habitats: Vec<HabitatView>,
    concessions: Vec<ConcessionView>,
    animal_care_depot: AnimalCareDepotView,
    animals: Vec<AnimalView>,
    guests: Vec<GuestView>,
    litter: Vec<LitterView>,
    maintenance: Vec<MaintenanceView>,
    work_zones: Vec<zone::WorkZoneView>,
    operations: OperationsView,
    species_catalog: Vec<SpeciesOfferView>,
    /// The fence types a habitat can be rebuilt with (#95).
    fence_catalog: Vec<content::FenceType>,
    /// Terrain prices, height limits and furnishings for habitat design (#97).
    habitat_environment: environment::EnvironmentCatalogView,
    complaints: ComplaintSummary,
    construction: construction::HistoryView,
    finance: FinanceView,
    tutorial: tutorial::TutorialView,
    scenario: scenario::ScenarioView,
    alerts: Vec<alerts::Alert>,
    summary: overlay::DailySummary,
}

#[derive(Serialize)]
struct ActionResult {
    ok: bool,
    message: String,
}

impl ActionResult {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }

    fn json(self) -> String {
        serde_json::to_string(&self).expect("action result serialization is infallible")
    }
}

/// The built-in scenarios offered by the New park picker, in order: id, title, summary, map
/// size, starting cash, whether keeper training runs, and objective titles.
#[wasm_bindgen]
pub fn scenario_catalog_json() -> String {
    scenario::catalog_json()
}

#[wasm_bindgen]
pub struct ZooGame {
    state: GameState,
    session: session::Session,
}

impl ZooGame {
    fn with_state(mut state: GameState) -> Self {
        state.update_alerts();
        let content = serde_json::to_string(state.catalog.as_ref())
            .expect("validated catalog serialization is infallible");
        let start = session::start_identity(&state.scenario, &content, &state.catalog.ruleset_id);
        Self {
            state,
            session: session::Session::new(start),
        }
    }

    fn drain_events(&mut self) {
        let events = std::mem::take(&mut self.state.pending_events);
        self.session.push_events(events);
    }

    /// Runs one authoritative command and appends it, with its result, to the session log.
    /// A frozen session refuses every command without touching state.
    fn command(
        &mut self,
        kind: &'static str,
        args: serde_json::Value,
        run: impl FnOnce(&mut GameState) -> ActionResult,
    ) -> String {
        let tick = self.state.elapsed_minutes;
        let result = if self.session.frozen {
            ActionResult::error(session::FROZEN_MESSAGE)
        } else {
            let result = run(&mut self.state);
            self.state.update_alerts();
            self.state.update_objectives();
            self.state.update_tutorial();
            self.state.tutorial.note_result(&result);
            result
        };
        self.session
            .record(tick, kind, args, result.ok, &result.message);
        self.drain_events();
        result.json()
    }

    /// Advances the simulation after checking authoritative invariants. Replay calls this
    /// with merged, unclamped minutes; the player-facing `tick` clamps first.
    fn advance(&mut self, minutes: u32) -> Option<ActionResult> {
        if self.session.frozen {
            return None;
        }
        let tick = self.state.elapsed_minutes;
        if self.session.pending_fault == Some(session::PendingFault::Panic) {
            let message = "Injected simulation panic".to_owned();
            let seq = self.session.record(
                tick,
                "advance",
                serde_json::json!({ "minutes": 0 }),
                false,
                &message,
            );
            self.session.fail(seq, tick, "panic", message, true);
            panic!("zoo-core: injected simulation panic (#87 fault injection)");
        }
        if let Some(violation) = self.state.invariant_violation() {
            let message = format!("Invariant violation: {violation}");
            let seq = self.session.record(
                tick,
                "advance",
                serde_json::json!({ "minutes": 0 }),
                false,
                &message,
            );
            let injected = self.session.injected_corruption;
            self.session
                .fail(seq, tick, "invariant_violation", message.clone(), injected);
            return Some(ActionResult::error(message));
        }
        if minutes == 0 {
            return None;
        }
        self.state.tick(minutes);
        self.state.update_objectives();
        self.state.update_tutorial();
        self.session.record_advance(tick, u64::from(minutes));
        self.drain_events();
        Some(ActionResult::ok(session::advance_message(u64::from(
            minutes,
        ))))
    }

    fn replay_entry(&mut self, kind: &str, args: &serde_json::Value) -> Result<String, String> {
        let number = |name: &str| -> Result<u32, String> {
            args[name]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| format!("{kind}: missing numeric argument {name}"))
        };
        let text = |name: &str| -> Result<String, String> {
            args[name]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{kind}: missing text argument {name}"))
        };
        let points = || -> Result<Vec<u32>, String> {
            args["points"]
                .as_array()
                .ok_or_else(|| format!("{kind}: missing points"))?
                .iter()
                .map(|value| value.as_u64().and_then(|value| u32::try_from(value).ok()))
                .collect::<Option<Vec<u32>>>()
                .ok_or_else(|| format!("{kind}: points must be grid coordinates"))
        };
        Ok(match kind {
            "build_path_stroke" => {
                self.build_path_stroke(points()?, number("width")?, text("access")?)
            }
            "build_plaza" => self.build_plaza(
                number("ax")?,
                number("ay")?,
                number("bx")?,
                number("by")?,
                text("access")?,
            ),
            "place_habitat_outline" => self.place_habitat_outline(points()?),
            "reshape_habitat" => self.reshape_habitat(number("habitat_id")?, points()?),
            "upgrade_fence" => self.upgrade_fence(number("habitat_id")?, text("fence")?),
            "undo_construction" => self.undo_construction(),
            "redo_construction" => self.redo_construction(),
            "place_concession" => self.place_concession(number("x")?, number("y")?, text("kind")?),
            "place_habitat_rect" => {
                self.place_habitat_rect(number("ax")?, number("ay")?, number("bx")?, number("by")?)
            }
            "place_habitat" => {
                let orientation = u8::try_from(number("orientation")?)
                    .map_err(|_| "place_habitat: orientation out of range".to_owned())?;
                self.place_habitat(number("x")?, number("y")?, orientation)
            }
            "bulldoze" => self.bulldoze(number("x")?, number("y")?),
            "adopt" => self.adopt(number("habitat_id")?, text("species")?),
            "rename_animal" => self.rename_animal(number("animal_id")?, text("name")?),
            "buy_animal_feed" => self.buy_animal_feed(),
            "hire_keeper" => self.hire_keeper(),
            "hire_janitor" => self.hire_janitor(),
            "hire_mechanic" => self.hire_mechanic(),
            "set_keeper_schedule" => self.set_keeper_schedule(
                number("keeper_id")?,
                text("categories")?,
                number("shift_start_minute")?,
                number("shift_end_minute")?,
            ),
            "refill_water" => self.refill_water(number("habitat_id")?),
            "clean_habitat" => self.clean_habitat(number("habitat_id")?),
            "add_shelter" => self.add_shelter(number("habitat_id")?),
            "paint_habitat_terrain" => {
                self.paint_habitat_terrain(number("habitat_id")?, points()?, text("surface")?)
            }
            "set_habitat_elevation" => {
                self.set_habitat_elevation(number("habitat_id")?, points()?, number("height")?)
            }
            "place_habitat_object" => self.place_habitat_object(
                number("habitat_id")?,
                text("object")?,
                number("x")?,
                number("y")?,
            ),
            "remove_habitat_object" => {
                self.remove_habitat_object(number("habitat_id")?, number("object_id")?)
            }
            "set_care_task" => self.set_care_task(
                number("habitat_id")?,
                text("category")?,
                number("first_minute_of_day")?,
                number("interval_minutes")?,
                number("priority")?,
            ),
            "set_service_gate" => self.set_service_gate(
                number("habitat_id")?,
                number("x")?,
                number("y")?,
                text("side")?,
            ),
            "create_work_zone" => {
                self.create_work_zone(number("ax")?, number("ay")?, number("bx")?, number("by")?)
            }
            "set_work_zone_bounds" => self.set_work_zone_bounds(
                number("zone_id")?,
                number("ax")?,
                number("ay")?,
                number("bx")?,
                number("by")?,
            ),
            "delete_work_zone" => self.delete_work_zone(number("zone_id")?),
            "assign_keeper_zone" => {
                self.assign_keeper_zone(number("keeper_id")?, number("zone_id")?)
            }
            "skip_tutorial" => self.skip_tutorial(),
            "restart_tutorial" => self.restart_tutorial(),
            "inject_fault" => self.inject_fault(&text("kind")?),
            "advance" => self
                .advance(number("minutes")?)
                .map(ActionResult::json)
                .ok_or_else(|| "advance: recorded entry produced no result".to_owned())?,
            other => return Err(format!("unknown recorded command kind {other}")),
        })
    }
}

impl ZooGame {
    /// A one-tile path stroke.
    pub fn place_path(&mut self, x: u32, y: u32) -> String {
        self.build_path_stroke(vec![x, y], 1, "public".to_owned())
    }
}

#[wasm_bindgen]
impl ZooGame {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::with_state(GameState::default())
    }

    pub fn content_json(&self) -> String {
        serde_json::to_string(self.state.catalog.as_ref())
            .expect("validated catalog serialization is infallible")
    }

    /// The built-in empty starter park on validated content.
    pub fn from_content_json(document: &str) -> Result<ZooGame, String> {
        let catalog = Arc::new(Catalog::parse(document)?);
        Self::from_scenario(catalog, scenario::DEFAULT_SCENARIO_ID)
    }

    fn from_scenario(catalog: Arc<Catalog>, scenario_id: &str) -> Result<ZooGame, String> {
        let scenario = scenario::Scenario::built_in(scenario_id)
            .ok_or_else(|| format!("unknown scenario {scenario_id}"))?;
        Ok(Self::with_state(scenario.create(catalog)?))
    }

    /// Replaces the active park with a fresh park from the built-in scenario `scenario_id`,
    /// on the active content. Returns `{ok, message}`; a scenario that fails validation
    /// leaves the current park and session untouched.
    pub fn start_scenario(&mut self, scenario_id: &str) -> String {
        match Self::from_scenario(Arc::clone(&self.state.catalog), scenario_id) {
            Ok(game) => {
                let title = game.state.scenario.title.clone();
                *self = game;
                ActionResult::ok(format!("New park started from “{title}”")).json()
            }
            Err(message) => {
                ActionResult::error(format!("Scenario {scenario_id} could not start: {message}"))
                    .json()
            }
        }
    }

    /// Rebuilds a park from validated content and a recorded diagnostics bundle by
    /// re-creating its recorded built-in scenario and re-executing its ordered command log.
    /// Fails closed when the content or scenario does not match the recorded starting
    /// identity, the log is incomplete, or any result diverges.
    pub fn replay(content_document: &str, session_json: &str) -> Result<ZooGame, String> {
        let bundle: serde_json::Value = serde_json::from_str(session_json)
            .map_err(|error| format!("malformed diagnostics bundle: {error}"))?;
        let scenario_id = bundle["start"]["scenario"]
            .as_str()
            .ok_or("diagnostics bundle records no starting scenario")?;
        let catalog = Arc::new(Catalog::parse(content_document)?);
        let mut game = ZooGame::from_scenario(catalog, scenario_id)?;
        if &bundle["start"] != game.session.start() {
            return Err("content does not match the recorded starting-state identity".to_owned());
        }
        let log = session::Session::parse_replay_log(&bundle)?;
        for (seq, (kind, args, ok, message)) in log.into_iter().enumerate() {
            let result = game.replay_entry(&kind, &args)?;
            let result: serde_json::Value = serde_json::from_str(&result)
                .map_err(|error| format!("seq {seq}: unreadable result: {error}"))?;
            if result["ok"] != ok || result["message"] != message.as_str() {
                return Err(format!(
                    "replay diverged at seq {seq} ({kind}): recorded {ok}/{message:?}, got {result}"
                ));
            }
        }
        Ok(game)
    }

    /// The complete authoritative park as a versioned `zoo-save` document (#88).
    pub fn save_json(&self) -> String {
        save::encode(&self.state)
    }

    /// Replaces the active park with a validated save and starts a fresh session at the saved
    /// simulation time. Returns `{ok, message}`, plus a stable `code` on failure; a failed load
    /// leaves the current park and session untouched.
    pub fn load_save(&mut self, document: &str) -> String {
        match save::decode(document) {
            Ok(state) => {
                let (day, minute) = (state.day, state.minute_of_day);
                let mut game = Self::with_state(state);
                game.session.mark_save_start(document);
                *self = game;
                ActionResult::ok(format!(
                    "Park loaded at day {day}, {:02}:{:02}",
                    minute / 60,
                    minute % 60
                ))
                .json()
            }
            Err(error) => serde_json::json!({
                "ok": false,
                "code": error.code,
                "message": error.message,
            })
            .to_string(),
        }
    }

    pub fn snapshot_json(&self) -> String {
        serde_json::to_string(&self.state.snapshot()).expect("snapshot serialization is infallible")
    }

    /// One management overlay (#94): `access`, `welfare`, `staff`, `cleanliness`,
    /// `facilities` or `viewing`; `null` for an unknown kind. Read-only.
    pub fn overlay_json(&self, kind: &str) -> String {
        overlay::OverlayKind::parse(kind).map_or_else(
            || "null".to_owned(),
            |kind| {
                serde_json::to_string(&self.state.overlay(kind))
                    .expect("overlay serialization is infallible")
            },
        )
    }

    /// The bounded session diagnostics bundle (`zoo-diagnostics/1`, at most 1 MB).
    pub fn diagnostics_json(&self) -> String {
        let state = &self.state;
        let ids = |iter: &mut dyn Iterator<Item = u32>| {
            let mut ids: Vec<u32> = iter.collect();
            ids.sort_unstable();
            ids
        };
        let clock = serde_json::json!({
            "tick": state.elapsed_minutes,
            "day": state.day,
            "minute_of_day": state.minute_of_day,
        });
        let entities = serde_json::json!({
            "habitats": ids(&mut state.habitats.iter().map(|item| item.id)),
            "concessions": ids(&mut state.concessions.iter().map(|item| item.id)),
            "keepers": ids(&mut state.keepers.iter().map(|item| item.id)),
            "janitors": ids(&mut state.janitors.iter().map(|item| item.id)),
            "mechanics": ids(&mut state.mechanics.iter().map(|item| item.id)),
            "guests": ids(&mut state.guests.iter().map(|item| item.id)),
            "work_zones": ids(&mut state.work_zones.iter().map(|item| item.id)),
        });
        let work = serde_json::json!({
            "simulated_minutes": state.elapsed_minutes,
            "guest_updates": state.work.guest_updates,
            "path_searches": state.work.path_searches.get(),
            "topology_revision": state.work.topology_revision,
            "route_invalidations": state.work.route_invalidations,
        });
        self.session.bundle(clock, entities, work)
    }

    /// `"running"` or `"frozen"`; a cheap per-tick query for the UI.
    pub fn session_status(&self) -> String {
        if self.session.frozen {
            "frozen"
        } else {
            "running"
        }
        .to_owned()
    }

    /// Test-only fault injection (#87). `"invariant"` corrupts real authoritative state so
    /// the invariant check catches it at the next advance; `"panic"` makes the next advance
    /// panic. The browser only exposes it behind `?zoo-fault-injection=1`.
    pub fn inject_fault(&mut self, kind: &str) -> String {
        let was_frozen = self.session.frozen;
        let args = serde_json::json!({ "kind": kind });
        let result = self.command("inject_fault", args, |state| match kind {
            "invariant" => {
                state.cash_cents = content::MAX_CENTS + 1;
                ActionResult::ok("Injected invariant fault: cash corrupted beyond its range")
            }
            "panic" => ActionResult::ok("Injected panic fault: the next advance panics"),
            other => ActionResult::error(format!("Unknown fault kind {other}")),
        });
        if !was_frozen {
            match kind {
                "invariant" => self.session.injected_corruption = true,
                "panic" => self.session.pending_fault = Some(session::PendingFault::Panic),
                _ => {}
            }
        }
        result
    }

    /// Previews a path stroke through `points` (flat `x, y` tile pairs) with a brush `width`
    /// tiles wide (1 to 3) and `access` `"public"` or `"staff"`, exactly as
    /// `build_path_stroke` would commit it now.
    pub fn evaluate_path_stroke(&self, points: Vec<u32>, width: u32, access: String) -> String {
        serde_json::to_string(&self.state.evaluate_path_stroke(&points, width, &access))
            .expect("construction preview serialization is infallible")
    }

    /// Builds one path stroke as a single all-or-nothing transaction.
    pub fn build_path_stroke(&mut self, points: Vec<u32>, width: u32, access: String) -> String {
        let args = serde_json::json!({ "points": points, "width": width, "access": access });
        self.command("build_path_stroke", args, |state| {
            state.build_path_stroke(&points, width, &access)
        })
    }

    /// Previews a rectangular plaza between two corner tiles (#98).
    pub fn evaluate_plaza(&self, ax: u32, ay: u32, bx: u32, by: u32, access: String) -> String {
        serde_json::to_string(&self.state.evaluate_plaza(ax, ay, bx, by, &access))
            .expect("construction preview serialization is infallible")
    }

    /// Builds a rectangular plaza as a single all-or-nothing transaction.
    pub fn build_plaza(&mut self, ax: u32, ay: u32, bx: u32, by: u32, access: String) -> String {
        let args = serde_json::json!({ "ax": ax, "ay": ay, "bx": bx, "by": by, "access": access });
        self.command("build_plaza", args, |state| {
            state.build_plaza(ax, ay, bx, by, &access)
        })
    }

    pub fn evaluate_concession(&self, x: u32, y: u32, kind: String) -> String {
        serde_json::to_string(&self.state.evaluate_concession(x, y, &kind))
            .expect("construction preview serialization is infallible")
    }

    pub fn evaluate_demolition(&self, x: u32, y: u32) -> String {
        serde_json::to_string(&self.state.evaluate_demolition(x, y))
            .expect("construction preview serialization is infallible")
    }

    /// Reverses the latest eligible construction transaction and refunds what it spent.
    pub fn undo_construction(&mut self) -> String {
        self.command("undo_construction", serde_json::json!({}), |state| {
            state.undo_construction()
        })
    }

    /// Re-applies the latest undone construction transaction at the same price and IDs.
    pub fn redo_construction(&mut self) -> String {
        self.command("redo_construction", serde_json::json!({}), |state| {
            state.redo_construction()
        })
    }

    pub fn place_concession(&mut self, x: u32, y: u32, kind: String) -> String {
        let args = serde_json::json!({ "x": x, "y": y, "kind": kind });
        self.command("place_concession", args, |state| {
            state.place_concession(x, y, &kind)
        })
    }

    pub fn evaluate_habitat_rect(&self, ax: u32, ay: u32, bx: u32, by: u32) -> String {
        serde_json::to_string(&self.state.evaluate_habitat_rect(ax, ay, bx, by))
            .expect("placement evaluation serialization is infallible")
    }

    pub fn place_habitat_rect(&mut self, ax: u32, ay: u32, bx: u32, by: u32) -> String {
        let args = serde_json::json!({ "ax": ax, "ay": ay, "bx": bx, "by": by });
        self.command("place_habitat_rect", args, |state| {
            state.place_habitat_rect(ax, ay, bx, by)
        })
    }

    /// Previews a habitat inside the closed fence outline through `points` (flat `x, y` grid
    /// corner pairs; corner `(x, y)` is the north-west corner of tile `(x, y)`).
    pub fn evaluate_habitat_outline(&self, points: Vec<u32>) -> String {
        serde_json::to_string(&self.state.evaluate_habitat_outline(&points))
            .expect("placement evaluation serialization is infallible")
    }

    /// Builds a habitat inside a closed fence outline as one transaction.
    pub fn place_habitat_outline(&mut self, points: Vec<u32>) -> String {
        let args = serde_json::json!({ "points": points });
        self.command("place_habitat_outline", args, |state| {
            state.place_habitat_outline(&points)
        })
    }

    /// Previews moving a habitat's fence onto a closed outline.
    pub fn evaluate_habitat_reshape(&self, habitat_id: u32, points: Vec<u32>) -> String {
        serde_json::to_string(&self.state.evaluate_habitat_reshape(habitat_id, &points))
            .expect("placement evaluation serialization is infallible")
    }

    /// Moves a habitat's fence onto a closed outline, keeping its animals, care and staff.
    pub fn reshape_habitat(&mut self, habitat_id: u32, points: Vec<u32>) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id, "points": points });
        self.command("reshape_habitat", args, |state| {
            state.reshape_habitat(habitat_id, &points)
        })
    }

    /// Previews rebuilding a habitat's whole fence as another catalog fence type.
    pub fn evaluate_fence_upgrade(&self, habitat_id: u32, fence: String) -> String {
        serde_json::to_string(&self.state.evaluate_fence_upgrade(habitat_id, &fence))
            .expect("construction preview serialization is infallible")
    }

    /// Rebuilds a habitat's whole fence as another type at full price, as one transaction.
    pub fn upgrade_fence(&mut self, habitat_id: u32, fence: String) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id, "fence": fence });
        self.command("upgrade_fence", args, |state| {
            state.upgrade_fence(habitat_id, &fence)
        })
    }

    pub fn evaluate_habitat(&self, x: u32, y: u32, orientation: u8) -> String {
        serde_json::to_string(&self.state.evaluate_habitat(
            x,
            y,
            HabitatOrientation::from_code(orientation),
        ))
        .expect("placement evaluation serialization is infallible")
    }

    pub fn place_habitat(&mut self, x: u32, y: u32, orientation: u8) -> String {
        let args = serde_json::json!({ "x": x, "y": y, "orientation": orientation });
        self.command("place_habitat", args, |state| {
            state.place_habitat(x, y, HabitatOrientation::from_code(orientation))
        })
    }

    pub fn bulldoze(&mut self, x: u32, y: u32) -> String {
        self.command("bulldoze", serde_json::json!({ "x": x, "y": y }), |state| {
            state.bulldoze(x, y)
        })
    }

    pub fn adopt(&mut self, habitat_id: u32, species: String) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id, "species": species });
        self.command("adopt", args, |state| state.adopt(habitat_id, &species))
    }

    /// Renames one animal by its stable ID; the name is trimmed and at most 24 characters.
    pub fn rename_animal(&mut self, animal_id: u32, name: String) -> String {
        let args = serde_json::json!({ "animal_id": animal_id, "name": name });
        self.command("rename_animal", args, |state| {
            state.rename_animal(animal_id, &name)
        })
    }

    pub fn buy_animal_feed(&mut self) -> String {
        self.command(
            "buy_animal_feed",
            serde_json::json!({}),
            GameState::buy_animal_feed,
        )
    }

    pub fn hire_keeper(&mut self) -> String {
        self.command("hire_keeper", serde_json::json!({}), GameState::hire_keeper)
    }

    pub fn hire_janitor(&mut self) -> String {
        self.command(
            "hire_janitor",
            serde_json::json!({}),
            GameState::hire_janitor,
        )
    }

    pub fn hire_mechanic(&mut self) -> String {
        self.command(
            "hire_mechanic",
            serde_json::json!({}),
            GameState::hire_mechanic,
        )
    }

    /// Sets which care categories (comma-separated keys) and daily hours a keeper works.
    pub fn set_keeper_schedule(
        &mut self,
        keeper_id: u32,
        categories: String,
        shift_start_minute: u32,
        shift_end_minute: u32,
    ) -> String {
        let args = serde_json::json!({
            "keeper_id": keeper_id,
            "categories": categories,
            "shift_start_minute": shift_start_minute,
            "shift_end_minute": shift_end_minute,
        });
        self.command("set_keeper_schedule", args, |state| {
            state.set_keeper_schedule(keeper_id, &categories, shift_start_minute, shift_end_minute)
        })
    }

    pub fn refill_water(&mut self, habitat_id: u32) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id });
        self.command("refill_water", args, |state| state.refill_water(habitat_id))
    }

    pub fn clean_habitat(&mut self, habitat_id: u32) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id });
        self.command("clean_habitat", args, |state| {
            state.clean_habitat(habitat_id)
        })
    }

    pub fn add_shelter(&mut self, habitat_id: u32) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id });
        self.command("add_shelter", args, |state| state.add_shelter(habitat_id))
    }

    /// Previews painting habitat tiles (flat `x, y` pairs) with a surface (#97).
    pub fn evaluate_habitat_terrain(
        &self,
        habitat_id: u32,
        points: Vec<u32>,
        surface: String,
    ) -> String {
        serde_json::to_string(
            &self
                .state
                .evaluate_habitat_terrain(habitat_id, &points, &surface),
        )
        .expect("environment preview serialization is infallible")
    }

    pub fn paint_habitat_terrain(
        &mut self,
        habitat_id: u32,
        points: Vec<u32>,
        surface: String,
    ) -> String {
        let args =
            serde_json::json!({ "habitat_id": habitat_id, "points": points, "surface": surface });
        self.command("paint_habitat_terrain", args, |state| {
            state.paint_habitat_terrain(habitat_id, &points, &surface)
        })
    }

    /// Previews setting habitat tiles to one elevation step.
    pub fn evaluate_habitat_elevation(
        &self,
        habitat_id: u32,
        points: Vec<u32>,
        height: u32,
    ) -> String {
        serde_json::to_string(
            &self
                .state
                .evaluate_habitat_elevation(habitat_id, &points, height),
        )
        .expect("environment preview serialization is infallible")
    }

    pub fn set_habitat_elevation(
        &mut self,
        habitat_id: u32,
        points: Vec<u32>,
        height: u32,
    ) -> String {
        let args =
            serde_json::json!({ "habitat_id": habitat_id, "points": points, "height": height });
        self.command("set_habitat_elevation", args, |state| {
            state.set_habitat_elevation(habitat_id, &points, height)
        })
    }

    /// Previews placing a shelter or enrichment item on one habitat tile.
    pub fn evaluate_habitat_object(
        &self,
        habitat_id: u32,
        object: String,
        x: u32,
        y: u32,
    ) -> String {
        serde_json::to_string(
            &self
                .state
                .evaluate_habitat_object(habitat_id, &object, x, y),
        )
        .expect("environment preview serialization is infallible")
    }

    pub fn place_habitat_object(
        &mut self,
        habitat_id: u32,
        object: String,
        x: u32,
        y: u32,
    ) -> String {
        let args =
            serde_json::json!({ "habitat_id": habitat_id, "object": object, "x": x, "y": y });
        self.command("place_habitat_object", args, |state| {
            state.place_habitat_object(habitat_id, &object, x, y)
        })
    }

    pub fn remove_habitat_object(&mut self, habitat_id: u32, object_id: u32) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id, "object_id": object_id });
        self.command("remove_habitat_object", args, |state| {
            state.remove_habitat_object(habitat_id, object_id)
        })
    }

    /// Overrides one recurring care task of one habitat (times, cadence and priority).
    pub fn set_care_task(
        &mut self,
        habitat_id: u32,
        category: String,
        first_minute_of_day: u32,
        interval_minutes: u32,
        priority: u32,
    ) -> String {
        let args = serde_json::json!({
            "habitat_id": habitat_id,
            "category": category,
            "first_minute_of_day": first_minute_of_day,
            "interval_minutes": interval_minutes,
            "priority": priority,
        });
        self.command("set_care_task", args, |state| {
            state.set_care_task(
                habitat_id,
                &category,
                first_minute_of_day,
                interval_minutes,
                priority,
            )
        })
    }

    /// Resolves a tapped tile to one of the habitat's fence segments for a gate-move preview.
    pub fn evaluate_service_gate(&self, habitat_id: u32, x: u32, y: u32) -> String {
        serde_json::to_string(&self.state.evaluate_service_gate(habitat_id, x, y))
            .expect("gate evaluation serialization is infallible")
    }

    /// Moves a habitat's service gate onto one of its own fence segments.
    pub fn set_service_gate(&mut self, habitat_id: u32, x: u32, y: u32, side: String) -> String {
        let args = serde_json::json!({ "habitat_id": habitat_id, "x": x, "y": y, "side": side });
        self.command("set_service_gate", args, |state| {
            state.set_service_gate(habitat_id, x, y, &side)
        })
    }

    /// Previews a new (`zone_id` 0) or edited work zone between two corner tiles.
    pub fn evaluate_work_zone(&self, zone_id: u32, ax: u32, ay: u32, bx: u32, by: u32) -> String {
        serde_json::to_string(&self.state.evaluate_work_zone(zone_id, ax, ay, bx, by))
            .expect("work zone evaluation serialization is infallible")
    }

    pub fn create_work_zone(&mut self, ax: u32, ay: u32, bx: u32, by: u32) -> String {
        let args = serde_json::json!({ "ax": ax, "ay": ay, "bx": bx, "by": by });
        self.command("create_work_zone", args, |state| {
            state.create_work_zone(ax, ay, bx, by)
        })
    }

    /// Resizes or moves a work zone to the rectangle between two corner tiles.
    pub fn set_work_zone_bounds(
        &mut self,
        zone_id: u32,
        ax: u32,
        ay: u32,
        bx: u32,
        by: u32,
    ) -> String {
        let args =
            serde_json::json!({ "zone_id": zone_id, "ax": ax, "ay": ay, "bx": bx, "by": by });
        self.command("set_work_zone_bounds", args, |state| {
            state.set_work_zone_bounds(zone_id, ax, ay, bx, by)
        })
    }

    pub fn delete_work_zone(&mut self, zone_id: u32) -> String {
        let args = serde_json::json!({ "zone_id": zone_id });
        self.command("delete_work_zone", args, |state| {
            state.delete_work_zone(zone_id)
        })
    }

    pub fn assign_keeper_zone(&mut self, keeper_id: u32, zone_id: u32) -> String {
        let args = serde_json::json!({ "keeper_id": keeper_id, "zone_id": zone_id });
        self.command("assign_keeper_zone", args, |state| {
            state.assign_keeper_zone(keeper_id, zone_id)
        })
    }

    /// Hides the keeper-training checklist; it never changes park state.
    pub fn skip_tutorial(&mut self) -> String {
        self.command(
            "skip_tutorial",
            serde_json::json!({}),
            GameState::skip_tutorial,
        )
    }

    /// Restarts the keeper-training checklist from the current park.
    pub fn restart_tutorial(&mut self) -> String {
        self.command(
            "restart_tutorial",
            serde_json::json!({}),
            GameState::restart_tutorial,
        )
    }

    pub fn tick(&mut self, minutes: u32) {
        let _ = self.advance(minutes.min(240));
    }
}

impl Default for ZooGame {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Buys feed and adds one more keeper to the park-wide zone covering the habitat.
    fn staff_habitat(state: &mut GameState, _habitat_id: u32) {
        assert!(state.buy_animal_feed().ok);
        state.hire_zoned_keeper();
    }

    fn test_habitat(x: u32, y: u32, width: u32, height: u32) -> Habitat {
        Habitat {
            id: 1,
            tiles: shape::rect_tiles(x, y, width, height),
            fence: Catalog::parse(content::DEFAULT_DOCUMENT)
                .expect("valid built-in catalog")
                .fences[0]
                .clone(),
            species: Some(Species::parse("capybara").expect("default catalog capybara")),
            animals: vec![animal::Animal {
                id: 1,
                species: "capybara".to_owned(),
                name: "Pip".to_owned(),
                sex: animal::Sex::Female,
                life_stage: animal::LifeStage::Adult,
                arrived_day: 1,
                cosmetic_seed: 0,
                position: Position {
                    x: x + width / 2,
                    y: y + height / 2,
                },
                activity: animal::Activity::Resting { until_minute: 0 },
                energy: 100,
                decisions: 0,
            }],
            welfare: 100,
            food: 100,
            water: 100,
            cleanliness: 100,
            environment: environment::Environment::default(),
            care_schedule: None,
            service_gate: FenceSegment {
                x,
                y,
                side: FenceSide::North,
            },
        }
    }

    #[test]
    fn path_placement_is_idempotent_and_charges_once() {
        let mut state = GameState::default();
        let before = state.cash_cents;

        assert!(state.place_path(5, ENTRANCE_Y).ok);
        assert_eq!(state.cash_cents, before - PATH_COST);
        assert_eq!(state.finance_today.construction_expense_cents, PATH_COST);

        assert!(state.place_path(5, ENTRANCE_Y).ok);
        assert_eq!(state.cash_cents, before - PATH_COST);
        assert_eq!(state.finance_today.construction_expense_cents, PATH_COST);
    }

    #[test]
    fn drawn_habitat_uses_variable_fence_and_shared_validation() {
        let mut state = GameState::default();
        let evaluation = state.evaluate_habitat_rect(3, 8, 7, 11);

        assert!(evaluation.ok);
        assert_eq!((evaluation.width, evaluation.height), (5, 4));
        assert_eq!(evaluation.occupied_tiles.len(), 20);
        assert_eq!(evaluation.fence_segments.len(), 18);
        assert_eq!(
            evaluation.cost_cents,
            HABITAT_BASE_COST + 20 * HABITAT_TILE_COST + 18 * FENCE_SEGMENT_COST
        );

        let before = state.cash_cents;
        assert!(state.place_habitat_rect(7, 11, 3, 8).ok);
        assert_eq!(state.habitats[0].capacity(), 6);
        assert_eq!(state.cash_cents, before - evaluation.cost_cents);

        let repeated = state.place_habitat_rect(3, 8, 7, 11);
        assert!(repeated.ok);
        assert_eq!(state.cash_cents, before - evaluation.cost_cents);
    }

    #[test]
    fn drawn_habitat_rejects_small_disconnected_and_overlapping_loops() {
        let mut state = GameState::default();

        let too_small = state.evaluate_habitat_rect(3, 8, 4, 9);
        assert!(!too_small.ok);
        assert!(too_small.message.contains("3×3"));

        let disconnected = state.evaluate_habitat_rect(10, 1, 13, 4);
        assert!(!disconnected.ok);
        assert_eq!(
            disconnected.message,
            "The fence needs at least one path along its outside edge"
        );

        assert!(state.place_habitat_rect(3, 8, 6, 10).ok);
        let overlap = state.evaluate_habitat_rect(4, 9, 8, 12);
        assert!(!overlap.ok);
        assert_eq!(overlap.message, "The enclosed area must be clear grass");
    }

    #[test]
    fn rejected_in_park_candidates_keep_their_preview_geometry() {
        let mut state = GameState::default();
        assert!(state.place_habitat_rect(3, 8, 6, 10).ok);

        let overlap = state.evaluate_habitat_rect(4, 9, 8, 12);
        let disconnected = state.evaluate_habitat_rect(10, 1, 13, 4);
        state.cash_cents = 0;
        let unaffordable = state.evaluate_habitat_rect(3, 3, 5, 6);

        for (evaluation, message) in [
            (&overlap, "The enclosed area must be clear grass"),
            (
                &disconnected,
                "The fence needs at least one path along its outside edge",
            ),
            (&unaffordable, "Not enough cash"),
        ] {
            assert!(!evaluation.ok);
            assert_eq!(evaluation.message, message);
            let (width, height) = (evaluation.width, evaluation.height);
            assert_eq!(
                evaluation.occupied_tiles.len(),
                (width * height) as usize,
                "{message}"
            );
            assert_eq!(
                evaluation.fence_segments,
                crate::shape::clockwise_fence(&crate::shape::rect_tiles(
                    evaluation.x,
                    evaluation.y,
                    width,
                    height
                ))
                .unwrap(),
                "{message}"
            );
        }
    }

    #[test]
    fn rejected_unbounded_candidates_materialize_no_geometry() {
        let state = GameState::default();
        for evaluation in [
            state.evaluate_habitat_rect(3, 8, 4, 9),
            state.evaluate_habitat_rect(0, 0, u32::MAX, u32::MAX),
            state.evaluate_habitat_rect(17, 10, 21, 12),
        ] {
            assert!(!evaluation.ok);
            assert!(evaluation.occupied_tiles.is_empty());
            assert!(evaluation.fence_segments.is_empty());
        }
    }

    #[test]
    fn releasing_a_rejected_candidate_changes_nothing() {
        let mut state = GameState::default();
        let before = serde_json::to_string(&state.snapshot()).expect("snapshot serializes");
        assert!(!state.place_habitat_rect(3, 5, 5, 8).ok);
        let after = serde_json::to_string(&state.snapshot()).expect("snapshot serializes");
        assert_eq!(before, after);
    }

    #[test]
    fn legacy_habitat_api_remains_compatible() {
        let mut state = GameState::default();
        let preview = state.evaluate_habitat(3, 8, HabitatOrientation::Horizontal);
        assert!(preview.ok);
        assert_eq!((preview.width, preview.height), (4, 3));
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
    }

    #[test]
    fn expanded_species_catalog_has_distinct_requirements() {
        let state = GameState::default();
        let catalog = state.species_catalog();
        assert_eq!(catalog.len(), 6);

        let elephant = Species::parse("elephant").expect("elephant exists");
        let penguin = Species::parse("penguin").expect("penguin exists");
        assert!(elephant.purchase_cost() > penguin.purchase_cost());
        assert!(
            elephant.welfare_profile().space_per_animal
                > penguin.welfare_profile().space_per_animal
        );
        assert!(
            penguin.welfare_profile().minimum_social_group
                > elephant.welfare_profile().minimum_social_group
        );
    }

    #[test]
    fn large_drawn_habitats_support_more_animals() {
        let mut state = GameState::default();
        for x in 5..=9 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        assert!(state.place_habitat_rect(5, 8, 9, 12).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert_eq!(state.habitats[0].capacity(), 8);

        for _ in 0..6 {
            assert!(state.adopt(habitat_id, "zebra").ok);
        }
        assert_eq!(state.habitats[0].animal_count(), 6);
        assert!(state.habitats[0].space_score() < 100);
    }

    #[test]
    fn animal_positions_roam_deterministically_inside_the_fence() {
        let mut first = GameState::default();
        let mut second = GameState::default();
        for state in [&mut first, &mut second] {
            assert!(state.place_habitat_rect(3, 8, 6, 11).ok);
            let id = state.habitats[0].id;
            staff_habitat(state, id);
            assert!(state.adopt(id, "capybara").ok);
            assert!(state.adopt(id, "capybara").ok);
        }

        assert_eq!(
            serde_json::to_string(&first.animal_views()).unwrap(),
            serde_json::to_string(&second.animal_views()).unwrap()
        );
        let before = serde_json::to_string(&first.animal_views()).unwrap();
        first.tick(2);
        let after = serde_json::to_string(&first.animal_views()).unwrap();
        assert_ne!(before, after);
        for animal in first.animal_views() {
            let habitat = &first.habitats[0];
            let (x, y, width, height) = shape::bounds(&habitat.tiles);
            assert!((x + 1..x + width - 1).contains(&animal.x));
            assert!((y + 1..y + height - 1).contains(&animal.y));
        }
    }

    #[test]
    fn viewing_footprint_matches_the_five_wide_three_deep_rule_on_every_side() {
        let state = GameState::default();
        let habitat = test_habitat(10, 5, 5, 5);

        let north = state.viewing_footprint(&habitat, Position { x: 12, y: 4 }, FenceSide::North);
        let expected_north: Vec<Position> = (5..=7)
            .flat_map(|y| (10..=14).map(move |x| Position { x, y }))
            .collect();
        assert_eq!(north, expected_north);

        let south = state.viewing_footprint(&habitat, Position { x: 12, y: 10 }, FenceSide::South);
        let expected_south: Vec<Position> = (7..=9)
            .flat_map(|y| (10..=14).map(move |x| Position { x, y }))
            .collect();
        assert_eq!(south, expected_south);

        let west = state.viewing_footprint(&habitat, Position { x: 9, y: 7 }, FenceSide::West);
        let expected_west: Vec<Position> = (5..=9)
            .flat_map(|y| (10..=12).map(move |x| Position { x, y }))
            .collect();
        assert_eq!(west, expected_west);

        let east = state.viewing_footprint(&habitat, Position { x: 15, y: 7 }, FenceSide::East);
        let expected_east: Vec<Position> = (5..=9)
            .flat_map(|y| (12..=14).map(move |x| Position { x, y }))
            .collect();
        assert_eq!(east, expected_east);
    }

    #[test]
    fn viewing_route_prefers_an_uncrowded_visible_path_tile() {
        let mut state = GameState::default();
        assert!(state.place_habitat_rect(3, 8, 7, 10).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        // Visible from the path at x 5 but not from the crowded tile at x 4.
        state.habitats[0].animals[0].position = Position { x: 6, y: 9 };
        assert!(state.place_path(5, ENTRANCE_Y).ok);
        state.try_spawn_guest();

        let mut template = state.guests[0].clone();
        state.guests.clear();
        template.state = GuestState::Viewing;
        template.target_habitat = habitat_id;
        template.x = 4;
        template.y = ENTRANCE_Y;
        template.route.clear();
        template.route_index = 0;
        template.viewing_minutes = 10;
        for id in 1..=2 {
            let mut guest = template.clone();
            guest.id = id;
            state.guests.push(guest);
        }

        let route = state
            .viewing_route(
                &state.habitats[0],
                Position {
                    x: ENTRANCE_X,
                    y: ENTRANCE_Y,
                },
            )
            .expect("an alternate visible viewpoint should remain reachable");
        assert_eq!(
            route.last(),
            Some(&Position {
                x: 5,
                y: ENTRANCE_Y
            })
        );
    }

    #[test]
    fn shared_path_tile_counts_viewers_from_both_neighboring_habitats() {
        let mut state = GameState {
            habitats: vec![
                test_habitat(3, 8, 5, 3),
                Habitat {
                    id: 2,
                    ..test_habitat(3, 4, 5, 3)
                },
            ],
            ..GameState::default()
        };
        state.set_tile(5, ENTRANCE_Y, TileKind::Path(PathAccess::Public));

        let mut guest = Guest {
            id: 1,
            x: 5,
            y: ENTRANCE_Y,
            happiness: 80,
            energy: 90,
            hunger: 0,
            thirst: 0,
            value_perception: 70,
            minutes_in_park: 0,
            target_habitat: 1,
            state: GuestState::Viewing,
            route: Vec::new(),
            route_index: 0,
            viewing_minutes: 10,
            arrival_steps: 0,
            bought_food: false,
            bought_drink: false,
            visited_habitats: vec![1],
            cleanliness_concern: false,
        };
        state.guests.push(guest.clone());
        guest.id = 2;
        guest.target_habitat = 2;
        guest.visited_habitats = vec![2];
        state.guests.push(guest.clone());
        guest.id = 3;
        state.guests.push(guest);

        let occupancy = state.viewing_occupancy();
        assert_eq!(occupancy.get(&(5, ENTRANCE_Y)), Some(&3));

        let first = state
            .viewing_spot_for_guest_with_occupancy(&state.guests[0], &occupancy)
            .expect("first habitat should use the shared path viewpoint");
        let second = state
            .viewing_spot_for_guest_with_occupancy(&state.guests[1], &occupancy)
            .expect("second habitat should use the shared path viewpoint");
        assert_eq!(first.occupancy, 3);
        assert_eq!(second.occupancy, 3);
        assert!(first.crowded());
        assert!(second.crowded());
    }

    #[test]
    fn over_capacity_viewing_applies_a_bounded_engagement_penalty() {
        let mut state = GameState::default();
        assert!(state.place_habitat_rect(3, 8, 7, 10).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.try_spawn_guest();

        let mut template = state.guests[0].clone();
        state.guests.clear();
        template.state = GuestState::Viewing;
        template.target_habitat = habitat_id;
        template.x = 4;
        template.y = ENTRANCE_Y;
        template.route.clear();
        template.route_index = 0;
        template.viewing_minutes = 10;
        template.minutes_in_park = VIEWING_PENALTY_INTERVAL_MINUTES;
        template.happiness = 80;
        template.value_perception = 70;
        for id in 1..=3 {
            let mut guest = template.clone();
            guest.id = id;
            state.guests.push(guest);
        }

        let spot = state
            .viewing_spot_for_guest(&state.guests[0])
            .expect("guest should occupy a viewing spot");
        assert_eq!(spot.visible_animals, 1);
        assert_eq!(spot.capacity, 2);
        assert_eq!(spot.occupancy, 3);
        assert!(spot.crowded());

        state.advance_viewing();

        assert_eq!(state.guests[0].happiness, 79);
        assert_eq!(state.guests[0].value_perception, 69);
        assert_eq!(
            state.guest_thought(&state.guests[0]),
            "It's too crowded to get a good view."
        );
    }

    #[test]
    fn guests_visibly_arrive_through_the_entrance() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "flamingo").ok);

        state.tick(24);
        assert_eq!(state.guests.len(), 1);
        assert_eq!(
            (state.guests[0].x, state.guests[0].y),
            (ENTRANCE_X, ENTRANCE_Y)
        );
        assert_eq!(state.guests[0].state, GuestState::Arriving);

        state.tick(6);
        assert!(matches!(
            state.guests[0].state,
            GuestState::Arriving | GuestState::WalkingToHabitat
        ));
    }

    #[test]
    fn engaged_guests_continue_to_distinct_reachable_habitats() {
        let mut state = GameState::default();
        assert!(state.place_habitat_rect(5, 5, 8, 7).ok);
        let first_id = state.habitats[0].id;
        staff_habitat(&mut state, first_id);
        assert!(state.adopt(first_id, "capybara").ok);

        assert!(state.place_path(10, ENTRANCE_Y).ok);
        assert!(state.place_habitat_rect(11, 5, 14, 7).ok);
        let second_id = state.habitats[1].id;
        staff_habitat(&mut state, second_id);
        assert!(state.upgrade_fence(second_id, "steel_mesh").ok);
        assert!(state.adopt(second_id, "giraffe").ok);
        for x in 4..=10 {
            assert!(state.place_path(x, ENTRANCE_Y + 1).ok);
        }

        let mut observed_two_habitats = false;
        for _ in 0..180 {
            state.tick(1);
            if let Some(guest) = state.guests.iter().find(|guest| guest.id == 1)
                && guest.visited_habitats.len() >= 2
            {
                assert_ne!(guest.visited_habitats[0], guest.visited_habitats[1]);
                observed_two_habitats = true;
                break;
            }
        }

        assert!(observed_two_habitats);
    }

    #[test]
    fn dirty_paths_reduce_guest_engagement_without_changing_routing() {
        let mut clean = GameState::default();
        assert!(clean.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = clean.habitats[0].id;
        staff_habitat(&mut clean, habitat_id);
        assert!(clean.adopt(habitat_id, "capybara").ok);
        clean.tick(24);

        let mut dirty = clean.clone();
        for x in 1..=3 {
            assert!(dirty.add_litter(Position { x, y: ENTRANCE_Y }));
        }
        assert!(dirty.park_cleanliness() < GUEST_CLEANLINESS_CONCERN_THRESHOLD);

        clean.tick(10);
        dirty.tick(10);

        let clean_guest = clean.guests.iter().find(|guest| guest.id == 1).unwrap();
        let dirty_guest = dirty.guests.iter().find(|guest| guest.id == 1).unwrap();
        assert!(dirty_guest.cleanliness_concern);
        assert!(dirty_guest.happiness < clean_guest.happiness);
        assert!(dirty_guest.value_perception < clean_guest.value_perception);
        assert_eq!(
            (dirty_guest.x, dirty_guest.y),
            (clean_guest.x, clean_guest.y)
        );
    }

    #[test]
    fn severe_needs_end_a_visit_before_another_reachable_habitat() {
        let mut state = GameState::default();
        assert!(state.place_habitat_rect(5, 5, 8, 7).ok);
        let first_id = state.habitats[0].id;
        staff_habitat(&mut state, first_id);
        assert!(state.adopt(first_id, "capybara").ok);

        assert!(state.place_path(10, ENTRANCE_Y).ok);
        assert!(state.place_habitat_rect(11, 5, 14, 7).ok);
        let second_id = state.habitats[1].id;
        staff_habitat(&mut state, second_id);
        assert!(state.upgrade_fence(second_id, "steel_mesh").ok);
        assert!(state.adopt(second_id, "giraffe").ok);
        for x in 4..=10 {
            assert!(state.place_path(x, ENTRANCE_Y + 1).ok);
        }

        for _ in 0..120 {
            state.tick(1);
            if state
                .guests
                .iter()
                .find(|guest| guest.id == 1)
                .is_some_and(|guest| guest.state == GuestState::Viewing)
            {
                break;
            }
        }

        let guest = state.guests.iter_mut().find(|guest| guest.id == 1).unwrap();
        guest.thirst = GUEST_CONTINUE_MAX_THIRST + 1;
        guest.viewing_minutes = 1;
        state.advance_viewing();

        let guest = state.guests.iter().find(|guest| guest.id == 1).unwrap();
        assert_eq!(guest.state, GuestState::WalkingToExit);
        assert_eq!(guest.visited_habitats.len(), 1);
    }

    #[test]
    fn concession_placement_requires_path_and_is_idempotent() {
        let mut state = GameState::default();
        let before = state.cash_cents;

        assert!(state.place_concession(1, ENTRANCE_Y - 1, "food").ok);
        assert_eq!(state.cash_cents, before - FOOD_STAND_BUILD_COST);
        assert_eq!(state.concessions.len(), 1);

        assert!(state.place_concession(1, ENTRANCE_Y - 1, "food").ok);
        assert_eq!(state.cash_cents, before - FOOD_STAND_BUILD_COST);
        assert_eq!(state.concessions.len(), 1);

        let disconnected = state.place_concession(12, 2, "drink");
        assert!(!disconnected.ok);
        assert_eq!(
            disconnected.message,
            "Concession stands must touch a public guest path"
        );
    }

    #[test]
    fn guests_buy_food_and_drinks_from_pathside_stands() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        assert!(state.place_concession(1, ENTRANCE_Y - 1, "drink").ok);
        assert!(state.place_concession(3, ENTRANCE_Y - 1, "food").ok);

        state.tick(40);

        let drink = state
            .concessions
            .iter()
            .find(|stand| stand.kind == ConcessionKind::Drink)
            .unwrap();
        let food = state
            .concessions
            .iter()
            .find(|stand| stand.kind == ConcessionKind::Food)
            .unwrap();
        assert_eq!(drink.total_sales, 1);
        assert_eq!(food.total_sales, 1);
        assert_eq!(
            state.finance_today.concession_income_cents,
            DRINK_PRICE + FOOD_PRICE
        );
        assert!(state.guests[0].bought_drink);
        assert!(state.guests[0].bought_food);
        assert!(state.guests[0].hunger < FOOD_BUY_THRESHOLD);
        assert!(state.guests[0].thirst < DRINK_BUY_THRESHOLD);
        assert!(!state.litter.is_empty());
    }

    #[test]
    fn litter_generation_is_deduplicated_per_path_tile() {
        let mut state = GameState::default();
        let tile = Position {
            x: 2,
            y: ENTRANCE_Y,
        };

        assert!(state.add_litter(tile));
        assert!(!state.add_litter(tile));
        assert_eq!(state.litter.len(), 1);
    }

    #[test]
    fn unstaffed_litter_degrades_cleanliness_and_rating_over_time() {
        let mut state = GameState::default();
        let clean_rating = state.rating();
        assert!(state.add_litter(Position {
            x: 2,
            y: ENTRANCE_Y,
        }));
        let dirty_rating = state.rating();

        assert!(state.park_cleanliness() < 100);
        assert!(dirty_rating < clean_rating);
        state.tick(30);
        assert!(state.oldest_litter_age_minutes() >= 30);
        assert!(state.park_cleanliness() < 88);
    }

    #[test]
    fn janitor_claims_oldest_reachable_litter_and_cleans_only_on_arrival() {
        let mut state = GameState::default();
        assert!(state.place_path(5, ENTRANCE_Y).ok);
        assert!(state.hire_janitor().ok);
        let older = Position {
            x: 5,
            y: ENTRANCE_Y,
        };
        let newer = Position {
            x: 1,
            y: ENTRANCE_Y,
        };
        assert!(state.add_litter(older));
        assert!(state.add_litter(newer));
        let older_id = state.litter[0].id;

        state.advance_janitor_work();

        assert_eq!(state.janitors[0].target_litter_id, Some(older_id));
        assert!(state.litter.iter().any(|task| task.id == older_id));
        assert_ne!(
            (state.janitors[0].x, state.janitors[0].y),
            (older.x, older.y)
        );

        for _ in 0..4 {
            state.advance_janitor_work();
        }

        assert!(!state.litter.iter().any(|task| task.id == older_id));
        assert!(state.janitors[0].tasks_completed >= 1);
    }

    #[test]
    fn unreachable_litter_remains_backlogged_and_reports_blocked() {
        let mut state = GameState::default();
        assert!(state.hire_janitor().ok);
        state.set_tile(10, 10, TileKind::Path(PathAccess::Public));
        assert!(state.add_litter(Position { x: 10, y: 10 }));

        state.advance_janitor_work();

        assert_eq!(state.litter.len(), 1);
        assert_eq!(state.janitors[0].target_litter_id, None);
        assert!(state.litter_status(&state.litter[0]).contains("Blocked"));
        assert!(state.janitor_status(&state.janitors[0]).contains("Blocked"));
    }

    #[test]
    fn path_edits_release_invalid_cleanup_assignments_and_protect_staff_tiles() {
        let mut state = GameState::default();
        assert!(state.place_path(5, ENTRANCE_Y).ok);
        assert!(state.place_path(6, ENTRANCE_Y).ok);
        assert!(state.hire_janitor().ok);
        assert!(state.add_litter(Position {
            x: 6,
            y: ENTRANCE_Y,
        }));
        state.assign_janitor_tasks();
        assert!(state.janitors[0].target_litter_id.is_some());

        let occupied = state.bulldoze(state.janitors[0].x, state.janitors[0].y);
        assert!(!occupied.ok);

        assert!(state.bulldoze(5, ENTRANCE_Y).ok);
        state.advance_janitor_work();
        assert_eq!(state.janitors[0].target_litter_id, None);
        assert_eq!(state.litter.len(), 1);
    }

    #[test]
    fn janitor_hiring_and_wages_are_categorized_in_finance_ledger() {
        let mut state = GameState::default();
        let before = state.cash_cents;
        assert!(state.hire_janitor().ok);
        assert_eq!(
            state.finance_today.janitor_hiring_expense_cents,
            JANITOR_HIRE_COST
        );

        state.charge_upkeep();

        assert_eq!(
            state.finance_today.janitor_wages_expense_cents,
            JANITOR_HOURLY_WAGE
        );
        assert_eq!(
            state.cash_cents,
            before - JANITOR_HIRE_COST - JANITOR_HOURLY_WAGE
        );
    }

    #[test]
    fn concession_wear_creates_one_maintenance_task_and_service_states() {
        let mut state = GameState::default();
        assert!(state.place_concession(1, ENTRANCE_Y - 1, "food").ok);
        state.concessions[0].condition = MAINTENANCE_SERVICE_THRESHOLD + 1;

        state.advance_concession_maintenance();
        assert_eq!(state.maintenance.len(), 1);
        assert_eq!(
            state.concession_service_state(&state.concessions[0]),
            "degraded"
        );

        state.advance_concession_maintenance();
        assert_eq!(state.maintenance.len(), 1);
        state.concessions[0].condition = MAINTENANCE_FAILURE_THRESHOLD;
        assert_eq!(
            state.concession_service_state(&state.concessions[0]),
            "failed"
        );
    }

    #[test]
    fn failed_concession_does_not_make_sales() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        assert!(state.place_concession(1, ENTRANCE_Y - 1, "drink").ok);
        state.concessions[0].condition = MAINTENANCE_FAILURE_THRESHOLD;

        state.tick(40);

        assert_eq!(state.concessions[0].total_sales, 0);
        assert_eq!(state.finance_today.concession_income_cents, 0);
    }

    #[test]
    fn mechanic_claims_oldest_reachable_task_and_repairs_only_on_arrival() {
        let mut state = GameState::default();
        for x in 5..=7 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        assert!(state.place_concession(6, ENTRANCE_Y - 1, "food").ok);
        state.concessions[0].condition = 50;
        assert!(state.ensure_maintenance_task(state.concessions[0].id));
        let task_id = state.maintenance[0].id;
        assert!(state.hire_mechanic().ok);
        let cash_before_repair = state.cash_cents;

        state.advance_mechanic_work();

        assert_eq!(state.mechanics[0].target_maintenance_id, Some(task_id));
        assert!(state.concessions[0].condition < 100);
        assert_eq!(state.finance_today.maintenance_repair_expense_cents, 0);

        for _ in 0..8 {
            state.advance_mechanic_work();
            if state.maintenance.is_empty() {
                break;
            }
        }

        assert!(state.maintenance.is_empty());
        assert_eq!(state.concessions[0].condition, 100);
        assert_eq!(state.mechanics[0].repairs_completed, 1);
        assert_eq!(
            state.finance_today.maintenance_repair_expense_cents,
            MAINTENANCE_REPAIR_COST
        );
        assert_eq!(
            state.cash_cents,
            cash_before_repair - MAINTENANCE_REPAIR_COST
        );
    }

    #[test]
    fn mechanic_chooses_reachable_service_tile_across_split_path_components() {
        let mut state = GameState::default();
        assert!(state.place_path(5, ENTRANCE_Y).ok);
        assert!(state.place_path(6, ENTRANCE_Y).ok);
        assert!(state.place_concession(6, ENTRANCE_Y - 1, "food").ok);

        // The north service tile is an isolated path component and appears before the
        // reachable south tile in neighbor order. Mechanics must consider both.
        state.set_tile(6, ENTRANCE_Y - 2, TileKind::Path(PathAccess::Public));
        state.concessions[0].condition = 50;
        let concession_id = state.concessions[0].id;
        assert!(state.ensure_maintenance_task(concession_id));
        let task_id = state.maintenance[0].id;
        assert!(state.hire_mechanic().ok);

        state.assign_mechanic_tasks();

        assert_eq!(state.mechanics[0].target_maintenance_id, Some(task_id));
        assert!(
            !state
                .maintenance_status(&state.maintenance[0])
                .contains("Blocked")
        );

        for _ in 0..8 {
            state.advance_mechanic_work();
            if state.maintenance.is_empty() {
                break;
            }
        }

        assert!(state.maintenance.is_empty());
        assert_eq!(state.concessions[0].condition, 100);
        assert_eq!(state.mechanics[0].repairs_completed, 1);
    }

    #[test]
    fn unreachable_maintenance_remains_backlogged_and_reports_blocked() {
        let mut state = GameState::default();
        assert!(state.hire_mechanic().ok);
        state.set_tile(10, 10, TileKind::Path(PathAccess::Public));
        assert!(state.place_concession(10, 9, "drink").ok);
        state.concessions[0].condition = 50;
        assert!(state.ensure_maintenance_task(state.concessions[0].id));

        state.advance_mechanic_work();

        assert_eq!(state.maintenance.len(), 1);
        assert_eq!(state.mechanics[0].target_maintenance_id, None);
        assert!(
            state
                .maintenance_status(&state.maintenance[0])
                .contains("Blocked")
        );
        assert!(
            state
                .mechanic_status(&state.mechanics[0])
                .contains("Blocked")
        );
    }

    #[test]
    fn path_and_concession_edits_release_invalid_mechanic_work() {
        let mut state = GameState::default();
        for x in 5..=7 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        assert!(state.place_concession(6, ENTRANCE_Y - 1, "food").ok);
        state.concessions[0].condition = 50;
        assert!(state.ensure_maintenance_task(state.concessions[0].id));
        assert!(state.hire_mechanic().ok);
        state.assign_mechanic_tasks();
        assert!(state.mechanics[0].target_maintenance_id.is_some());

        let occupied = state.bulldoze(state.mechanics[0].x, state.mechanics[0].y);
        assert!(!occupied.ok);

        assert!(state.bulldoze(5, ENTRANCE_Y).ok);
        assert_eq!(state.mechanics[0].target_maintenance_id, None);
        assert_eq!(state.maintenance.len(), 1);

        assert!(state.bulldoze(6, ENTRANCE_Y - 1).ok);
        assert!(state.maintenance.is_empty());
    }

    #[test]
    fn mechanic_hiring_wages_and_repairs_are_categorized() {
        let mut state = GameState::default();
        assert!(state.place_concession(1, ENTRANCE_Y - 1, "food").ok);
        let before_hire = state.cash_cents;
        assert!(state.hire_mechanic().ok);
        assert_eq!(
            state.finance_today.mechanic_hiring_expense_cents,
            MECHANIC_HIRE_COST
        );
        state.charge_upkeep();
        assert_eq!(
            state.finance_today.mechanic_wages_expense_cents,
            MECHANIC_HOURLY_WAGE
        );
        assert_eq!(
            state.cash_cents,
            before_hire
                - MECHANIC_HIRE_COST
                - MECHANIC_HOURLY_WAGE
                - state.finance_today.park_upkeep_expense_cents
        );

        state.concessions[0].condition = 50;
        assert!(state.ensure_maintenance_task(state.concessions[0].id));
        for _ in 0..4 {
            state.advance_mechanic_work();
            if state.maintenance.is_empty() {
                break;
            }
        }
        assert_eq!(
            state.finance_today.maintenance_repair_expense_cents,
            MAINTENANCE_REPAIR_COST
        );
    }

    #[test]
    fn mechanic_maintenance_loop_is_deterministic() {
        let mut first = GameState::default();
        let mut second = GameState::default();
        for state in [&mut first, &mut second] {
            assert!(state.place_concession(1, ENTRANCE_Y - 1, "food").ok);
            assert!(state.hire_mechanic().ok);
            state.concessions[0].condition = 50;
            assert!(state.ensure_maintenance_task(state.concessions[0].id));
            state.tick(30);
        }
        assert_eq!(
            serde_json::to_string(&first.snapshot()).unwrap(),
            serde_json::to_string(&second.snapshot()).unwrap()
        );
    }

    /// Two empty habitats on one path row; neither generates jobs of its own.
    fn empty_two_habitat_park() -> GameState {
        let mut state = GameState::default();
        for x in 5..=11 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        assert!(state.place_habitat_rect(3, 8, 5, 10).ok);
        assert!(state.place_habitat_rect(7, 8, 9, 10).ok);
        state
    }

    /// Adds a due inspection job (no supplies) directly, as a habitat schedule would.
    fn push_inspection(state: &mut GameState, habitat_id: u32, priority: u32, age: u64) -> u32 {
        let id = state.next_care_job_id;
        state.next_care_job_id += 1;
        state.care_jobs.push(care::CareJob {
            id,
            habitat_id,
            category: content::CareCategory::Inspection,
            due_minute: state.absolute_minute() - age,
            priority,
            required_keepers: 1,
            work_minutes: 10,
            state: care::CareJobState::Due,
            keeper_ids: Vec::new(),
            supplies_reserved: false,
            resolved_minute: None,
        });
        id
    }

    fn job(state: &GameState, id: u32) -> care::CareJob {
        state
            .care_jobs
            .iter()
            .find(|job| job.id == id)
            .unwrap()
            .clone()
    }

    fn waiting_reason(state: &GameState, habitat_id: u32, id: u32) -> Option<String> {
        state
            .care_job_views(habitat_id)
            .into_iter()
            .find(|view| view.id == id)
            .and_then(|view| view.waiting_reason)
    }

    #[test]
    fn short_staffing_serves_priority_first_and_leaves_lower_work_missed() {
        let mut state = empty_two_habitat_park();
        state.hire_zoned_keeper();
        let low = push_inspection(&mut state, 1, 2, 50);
        let high = push_inspection(&mut state, 2, 4, 0);

        state.tick(1);
        assert_eq!(job(&state, high).state, care::CareJobState::InProgress);
        assert_eq!(job(&state, high).keeper_ids, [1]);
        assert_eq!(job(&state, low).state, care::CareJobState::Due);
        assert_eq!(
            waiting_reason(&state, 1, low).as_deref(),
            Some("All zone #1 inspection keepers are busy")
        );

        // The only keeper is still walking or working when the low job's grace hour lapses.
        state.tick(10);
        assert_eq!(job(&state, low).state, care::CareJobState::Missed);
        assert_eq!(job(&state, low).keeper_ids, Vec::<u32>::new());
        assert_eq!(waiting_reason(&state, 1, low), None);
        state.tick(60);
        assert_eq!(job(&state, high).state, care::CareJobState::Completed);
        assert_eq!(job(&state, low).state, care::CareJobState::Missed);
    }

    #[test]
    fn equal_priority_work_resolves_oldest_due_first() {
        let mut state = empty_two_habitat_park();
        state.hire_zoned_keeper();
        let newer = push_inspection(&mut state, 1, 3, 5);
        let older = push_inspection(&mut state, 2, 3, 20);

        state.tick(1);
        assert_eq!(job(&state, older).keeper_ids, [1]);
        assert_eq!(job(&state, newer).state, care::CareJobState::Due);
    }

    #[test]
    fn a_job_takes_the_nearest_free_keeper_and_never_double_books() {
        let mut state = empty_two_habitat_park();
        state.hire_zoned_keeper();
        state.hire_zoned_keeper();
        // Keeper #2 is already standing at habitat #2's gate access tile.
        let access = state
            .gate_access_tile(state.habitats[1].service_gate)
            .unwrap();
        state.keepers[1].x = access.x;
        state.keepers[1].y = access.y;
        state.keepers[1].activity = care::KeeperActivity::ToDepot;
        let near = push_inspection(&mut state, 2, 3, 0);
        let other = push_inspection(&mut state, 1, 3, 0);
        let third = push_inspection(&mut state, 1, 3, 0);

        state.tick(1);
        assert_eq!(job(&state, near).keeper_ids, [2]);
        assert_eq!(job(&state, other).keeper_ids, [1]);
        assert_eq!(job(&state, third).state, care::CareJobState::Due);
        let busy: Vec<u32> = state
            .care_jobs
            .iter()
            .filter(|job| job.state == care::CareJobState::InProgress)
            .flat_map(|job| job.keeper_ids.clone())
            .collect();
        assert_eq!(busy, vec![2, 1]);
    }

    #[test]
    fn eligibility_follows_zone_category_and_shift_without_borrowing() {
        let mut state = empty_two_habitat_park();
        // Habitat #1's gate (3, 8) lies in zone #1; zone #2 covers the rest of the park.
        assert!(state.create_work_zone(0, 0, 5, 13).ok);
        assert!(state.create_work_zone(6, 0, 19, 13).ok);
        assert_eq!(state.habitat_zone(&state.habitats[0]), Some(1));
        assert!(state.hire_keeper().ok);
        let id = push_inspection(&mut state, 1, 3, 0);

        state.tick(1);
        assert_eq!(
            waiting_reason(&state, 1, id).as_deref(),
            Some("No keeper works in zone #1; assign one in the zone inspector")
        );
        // A free keeper in another zone is never borrowed.
        assert!(state.assign_keeper_zone(1, 2).ok);
        state.tick(1);
        assert_eq!(job(&state, id).state, care::CareJobState::Due);

        assert!(state.assign_keeper_zone(1, 1).ok);
        assert!(state.set_keeper_schedule(1, "cleaning", 0, 1440).ok);
        state.tick(1);
        assert_eq!(
            waiting_reason(&state, 1, id).as_deref(),
            Some("No zone #1 keeper accepts inspection")
        );

        // 09:03 now; a 13:00–05:00 shift wraps past midnight and excludes the morning.
        assert!(
            state
                .set_keeper_schedule(1, "inspection,cleaning", 780, 300)
                .ok
        );
        assert_eq!(
            state.keepers[0].schedule.categories,
            vec![
                content::CareCategory::Cleaning,
                content::CareCategory::Inspection
            ]
        );
        state.tick(1);
        assert_eq!(
            waiting_reason(&state, 1, id).as_deref(),
            Some("No zone #1 inspection keeper is on shift")
        );
        assert_eq!(
            state.keeper_view(&state.keepers[0]).status,
            "Off shift until 13:00"
        );

        assert!(state.set_keeper_schedule(1, "inspection", 540, 1020).ok);
        state.tick(1);
        assert_eq!(job(&state, id).keeper_ids, [1]);
    }

    #[test]
    fn keeper_schedules_are_validated_and_idempotent() {
        let mut state = GameState::default();
        assert!(state.hire_keeper().ok);
        for (categories, start, end) in [
            ("", 0, 1440),
            ("grooming", 0, 1440),
            ("feeding", 1440, 1440),
            ("feeding", 0, 0),
            ("feeding", 600, 600),
            ("feeding", 0, 1441),
        ] {
            assert!(
                !state.set_keeper_schedule(1, categories, start, end).ok,
                "{categories} {start}-{end}"
            );
        }
        assert!(!state.set_keeper_schedule(9, "feeding", 0, 1440).ok);
        assert!(
            state
                .set_keeper_schedule(1, "feeding,feeding,cleaning", 360, 1080)
                .ok
        );
        let repeat = state.set_keeper_schedule(1, "cleaning, feeding", 360, 1080);
        assert!(repeat.ok);
        assert!(repeat.message.contains("already"));
    }

    #[test]
    fn reassigning_a_keeper_mid_job_keeps_the_running_job_intact() {
        let mut state = empty_two_habitat_park();
        assert!(state.create_work_zone(0, 0, 5, 13).ok);
        assert!(state.create_work_zone(6, 0, 19, 13).ok);
        assert!(state.hire_keeper().ok);
        assert!(state.assign_keeper_zone(1, 1).ok);
        let first = push_inspection(&mut state, 1, 3, 0);
        state.tick(1);
        assert_eq!(job(&state, first).keeper_ids, [1]);

        assert!(state.assign_keeper_zone(1, 2).ok);
        assert!(state.set_keeper_schedule(1, "feeding", 0, 1440).ok);
        let second = push_inspection(&mut state, 1, 3, 0);
        state.tick(60);
        assert_eq!(job(&state, first).state, care::CareJobState::Completed);
        assert_eq!(job(&state, first).keeper_ids, [1]);
        // Future zone #1 inspection work is no longer this keeper's.
        assert_eq!(job(&state, second).state, care::CareJobState::Missed);
        assert!(state.invariant_violation().is_none());
    }

    #[test]
    fn scheduled_delivery_consumes_depot_stock_and_refills_food() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.habitats[0].food = 40;
        let before = state.feed_crates;

        let feeding = |state: &GameState| {
            state
                .care_jobs
                .iter()
                .find(|job| job.category == content::CareCategory::Feeding)
                .expect("feeding job generated")
                .clone()
        };

        // Capybara feeding falls due every two hours from 07:00; the park opens at 09:00. The
        // keeper takes a crate at dispatch, so the depot stock drops before food is delivered.
        state.tick(1);
        assert_eq!(feeding(&state).state, care::CareJobState::InProgress);
        assert_eq!(state.feed_crates, before - 1);
        assert!(state.habitats[0].food < 100);

        // Depot -> (2, 7) -> gate access (3, 7) -> inside (3, 8): three 3-minute steps, then
        // fifteen minutes of feeding.
        let mut minutes = 1;
        while feeding(&state).state != care::CareJobState::Completed {
            assert_eq!(state.keepers[0].deliveries_completed, 0);
            if matches!(
                state.keepers[0].activity,
                care::KeeperActivity::Working { .. }
            ) {
                assert_eq!((state.keepers[0].x, state.keepers[0].y), (3, 8));
            }
            state.tick(1);
            minutes += 1;
            assert!(minutes <= 30, "feeding never completed");
        }
        assert!(minutes > 3 * 2 + 15, "completed after {minutes} min");
        assert_eq!(state.habitats[0].food, 100);
        assert_eq!(state.keepers[0].deliveries_completed, 1);

        // Nothing else is due, so the keeper walks back out through the gate to the depot.
        state.tick(12);
        assert_eq!(state.keepers[0].activity, care::KeeperActivity::AtDepot);
        assert_eq!(
            (state.keepers[0].x, state.keepers[0].y),
            (ANIMAL_CARE_DEPOT_X, ANIMAL_CARE_DEPOT_Y)
        );
    }

    #[test]
    fn empty_depot_leaves_feeding_due_then_missed_without_free_feed() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        state.hire_zoned_keeper();
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.habitats[0].food = 40;
        let feeding = |state: &GameState| {
            state
                .care_jobs
                .iter()
                .find(|job| job.category == content::CareCategory::Feeding)
                .expect("feeding job generated")
                .clone()
        };

        state.tick(1);
        assert_eq!(feeding(&state).state, care::CareJobState::Due);
        assert!(state.habitats[0].food < 100);

        // Due at 09:00 and still eligible until 09:59.
        state.tick(58);
        assert_eq!(feeding(&state).state, care::CareJobState::Due);
        state.tick(1);
        assert_eq!(feeding(&state).state, care::CareJobState::Missed);
        assert_eq!(state.feed_crates, 0);
        assert!(state.habitats[0].food < 100);
        assert!(
            state
                .pending_events
                .iter()
                .any(|event| event.kind == "care_job_missed")
        );
    }

    fn feeding_job(state: &GameState) -> care::CareJob {
        state
            .care_jobs
            .iter()
            .filter(|job| job.category == content::CareCategory::Feeding)
            .min_by_key(|job| job.id)
            .expect("feeding job generated")
            .clone()
    }

    #[test]
    fn unreachable_gate_blocks_care_and_flags_the_habitat() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.habitats[0].food = 40;
        // Losing the only path between depot and gate is allowed, not rejected.
        assert!(state.bulldoze(3, ENTRANCE_Y).ok);
        assert!(state.bulldoze(2, ENTRANCE_Y).ok);

        let view = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(view["habitats"][0]["service_gate"]["reachable"], false);
        assert_eq!(view["habitats"][0]["care_status"], GATE_UNREACHABLE);
        assert!(GATE_UNREACHABLE.starts_with("Gate unreachable"));

        state.tick(60);
        assert_eq!(feeding_job(&state).state, care::CareJobState::Missed);
        assert_eq!(state.keepers[0].activity, care::KeeperActivity::AtDepot);
        assert_eq!(state.feed_crates, FEED_BATCH_CRATES);
    }

    #[test]
    fn a_job_dispatched_inside_the_grace_hour_runs_to_completion() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.habitats[0].food = 40;
        assert!(state.bulldoze(2, ENTRANCE_Y).ok);

        // Due at 09:00; the route returns at 09:50, ten minutes before the job would expire.
        state.tick(50);
        assert_eq!(feeding_job(&state).state, care::CareJobState::Due);
        assert!(state.place_path(2, ENTRANCE_Y).ok);
        state.tick(1);
        assert_eq!(feeding_job(&state).state, care::CareJobState::InProgress);
        assert_eq!(feeding_job(&state).keeper_ids, [1]);

        // Walking plus fifteen minutes of work end well after the grace hour.
        state.tick(20);
        assert_eq!(feeding_job(&state).state, care::CareJobState::InProgress);
        state.tick(10);
        assert_eq!(feeding_job(&state).state, care::CareJobState::Completed);
        assert!(state.habitats[0].food >= 99, "{}", state.habitats[0].food);
    }

    #[test]
    fn demolishing_a_habitat_mid_walk_returns_the_keeper_and_its_feed() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.habitats[0].food = 40;
        state.tick(4);
        assert!(matches!(
            state.keepers[0].activity,
            care::KeeperActivity::ToGate { .. }
        ));
        assert_eq!(state.feed_crates, FEED_BATCH_CRATES - 1);

        assert!(state.bulldoze(3, 8).ok);
        assert_eq!(state.feed_crates, FEED_BATCH_CRATES);
        assert_eq!(state.keepers[0].activity, care::KeeperActivity::AtDepot);
        assert_eq!(
            (state.keepers[0].x, state.keepers[0].y),
            (ANIMAL_CARE_DEPOT_X, ANIMAL_CARE_DEPOT_Y)
        );
        assert!(state.care_jobs.is_empty());
    }

    #[test]
    fn a_keeper_follows_a_moved_gate_and_publishes_its_route() {
        let mut state = GameState::default();
        for x in 5..=7 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        for y in 8..=10 {
            assert!(state.place_path(7, y).ok);
        }
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        assert!(state.set_service_gate(habitat_id, 6, 10, "east").ok);
        state.habitats[0].food = 40;
        state.tick(1);

        let view = serde_json::to_value(state.snapshot()).unwrap();
        let keeper = &view["animal_care_depot"]["keepers"][0];
        assert_eq!(keeper["activity"], "to_gate");
        let route: Vec<(u64, u64)> = keeper["route"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["x"].as_u64().unwrap(), p["y"].as_u64().unwrap()))
            .collect();
        assert_eq!(route.first(), Some(&(2, 6)));
        assert_eq!(&route[route.len() - 2..], &[(7, 10), (6, 10)]);

        while !matches!(
            state.keepers[0].activity,
            care::KeeperActivity::Working { .. }
        ) {
            state.tick(1);
        }
        assert_eq!((state.keepers[0].x, state.keepers[0].y), (6, 10));
        assert_eq!(state.keepers[0].exit, Some(Position { x: 7, y: 10 }));
    }

    fn stocked_habitat(state: &mut GameState, x: u32, species: &str) -> u32 {
        assert!(state.place_habitat_rect(x, 8, x + 2, 10).ok);
        let habitat_id = state.habitats.last().unwrap().id;
        staff_habitat(state, habitat_id);
        if matches!(species, "elephant" | "giraffe") {
            assert!(state.upgrade_fence(habitat_id, "steel_mesh").ok);
        }
        assert!(state.adopt(habitat_id, species).ok);
        habitat_id
    }

    fn two_habitat_park() -> GameState {
        let mut state = GameState::default();
        for x in 5..=11 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        stocked_habitat(&mut state, 3, "zebra");
        stocked_habitat(&mut state, 7, "zebra");
        state
    }

    #[test]
    fn new_habitat_instantiates_species_default_schedule() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        assert!(state.habitats[0].care_schedule.is_none());
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.upgrade_fence(habitat_id, "steel_mesh").ok);
        assert!(state.adopt(habitat_id, "elephant").ok);

        let elephant = Species::parse("elephant").unwrap();
        let schedule = state.habitats[0].care_schedule.as_ref().unwrap();
        assert_eq!(schedule.required_keepers, elephant.care.required_keepers);
        assert_eq!(schedule.tasks.len(), elephant.care.schedule.len());
        for (task, default) in schedule.tasks.iter().zip(&elephant.care.schedule) {
            assert_eq!(task.category, default.category);
            assert_eq!(task.first_minute_of_day, default.first_minute_of_day);
            assert_eq!(task.interval_minutes, default.interval_minutes);
            assert_eq!(task.priority, default.priority);
            assert!(!task.customized);
        }

        // A full day without manual configuration generates every category exactly as often
        // as its cadence implies.
        state.tick(24 * 60);
        for default in &elephant.care.schedule {
            let generated = state
                .care_jobs
                .iter()
                .filter(|job| job.category == default.category)
                .count() as u32;
            assert!(
                generated >= 24 * 60 / default.interval_minutes,
                "{default:?}"
            );
        }
    }

    #[test]
    fn overriding_one_habitat_leaves_other_habitats_and_catalog_untouched() {
        let mut state = two_habitat_park();
        let catalog_before = state.catalog.species.clone();
        let other_before = state.habitats[1].care_schedule.clone();

        let result = state.set_care_task(1, "cleaning", 0, 240, 5);
        assert!(result.ok, "{}", result.message);
        assert!(state.set_care_task(1, "cleaning", 0, 240, 5).ok);

        let task = state.habitats[0]
            .care_schedule
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|task| task.category == content::CareCategory::Cleaning)
            .copied()
            .unwrap();
        assert_eq!((task.first_minute_of_day, task.interval_minutes), (0, 240));
        assert_eq!(task.priority, 5);
        assert!(task.customized);
        assert_eq!(state.habitats[1].care_schedule, other_before);
        assert_eq!(state.catalog.species, catalog_before);

        assert!(!state.set_care_task(1, "cleaning", 0, 25, 3).ok);
        assert!(!state.set_care_task(1, "cleaning", 0, 240, 9).ok);
        assert!(!state.set_care_task(1, "grooming", 0, 240, 3).ok);
        assert!(!state.set_care_task(1, "enrichment", 0, 240, 3).ok);
    }

    #[test]
    fn rescheduling_never_duplicates_an_occurrence() {
        let mut state = two_habitat_park();
        state.tick(1);
        let feeding_jobs = |state: &GameState| {
            state
                .care_jobs
                .iter()
                .filter(|job| job.habitat_id == 1 && job.category == content::CareCategory::Feeding)
                .count()
        };
        let before = feeding_jobs(&state);
        // Re-anchoring so the current minute is an occurrence again must not re-issue it.
        assert!(state.set_care_task(1, "feeding", 540, 120, 2).ok);
        assert!(state.set_care_task(1, "feeding", 420, 120, 4).ok);
        state.tick(1);
        assert_eq!(feeding_jobs(&state), before);

        let mut keys: Vec<_> = state
            .care_jobs
            .iter()
            .map(|job| (job.habitat_id, job.category, job.due_minute))
            .collect();
        let total = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), total);
    }

    fn habitat_view(state: &GameState, habitat_id: u32) -> serde_json::Value {
        let view = serde_json::to_value(state.snapshot()).unwrap();
        view["habitats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|habitat| habitat["id"] == habitat_id)
            .unwrap()
            .clone()
    }

    #[test]
    fn one_keeper_cannot_begin_dangerous_work_which_is_missed_after_the_grace_hour() {
        let mut state = GameState::default();
        for x in 5..=11 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        let elephants = stocked_habitat(&mut state, 3, "elephant");
        let zebras = stocked_habitat(&mut state, 7, "zebra");
        // Two zone keepers, but only keeper #1 is ever on shift.
        assert!(state.set_keeper_schedule(2, "inspection", 0, 60).ok);
        let habitat = habitat_view(&state, elephants);
        assert_eq!(habitat["care_required_keepers"], 2);
        let id = push_inspection(&mut state, elephants, 5, 0);
        state.care_jobs.last_mut().unwrap().required_keepers = 2;

        state.tick(1);
        assert_eq!(job(&state, id).state, care::CareJobState::Due);
        assert!(job(&state, id).keeper_ids.is_empty());
        assert_eq!(
            waiting_reason(&state, elephants, id).as_deref(),
            Some("Needs 2 keepers together; only 1 zone #1 inspection keeper is on shift")
        );
        // Ordinary-species care still runs with its single keeper.
        state.tick(30);
        assert!(
            state
                .care_jobs
                .iter()
                .any(|job| job.habitat_id == zebras && job.state != care::CareJobState::Due)
        );
        state.tick(60);
        assert_eq!(job(&state, id).state, care::CareJobState::Missed);
        assert!(
            state
                .care_jobs
                .iter()
                .filter(|job| job.habitat_id == elephants)
                .all(|job| job.keeper_ids.is_empty() && job.state != care::CareJobState::Completed)
        );
    }

    #[test]
    fn a_lone_zone_keeper_shows_the_dangerous_crew_shortage() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.upgrade_fence(habitat_id, "steel_mesh").ok);
        assert!(state.adopt(habitat_id, "elephant").ok);
        let habitat = habitat_view(&state, habitat_id);
        assert_eq!(
            habitat["care_status"],
            "Dangerous animals: every job needs 2 keepers together, but only 1 works in zone #1"
        );
        state.hire_zoned_keeper();
        let habitat = habitat_view(&state, habitat_id);
        assert!(
            !habitat["care_status"]
                .as_str()
                .unwrap()
                .starts_with("Dangerous")
        );
    }

    /// An elephant habitat with two zone keepers and one due crew inspection.
    fn crew_park() -> (GameState, u32, u32) {
        let mut state = empty_two_habitat_park();
        state.hire_zoned_keeper();
        state.hire_zoned_keeper();
        let habitat_id = state.habitats[0].id;
        assert!(state.upgrade_fence(habitat_id, "steel_mesh").ok);
        assert!(state.adopt(habitat_id, "elephant").ok);
        // Only the pushed job runs, so the crew's work is easy to follow.
        state.habitats[0]
            .care_schedule
            .as_mut()
            .unwrap()
            .tasks
            .clear();
        state.care_jobs.clear();
        let id = push_inspection(&mut state, habitat_id, 5, 0);
        state.care_jobs.last_mut().unwrap().required_keepers = 2;
        (state, habitat_id, id)
    }

    #[test]
    fn two_keepers_reserve_synchronize_and_complete_exactly_one_job() {
        let (mut state, habitat_id, id) = crew_park();
        state.tick(1);
        assert_eq!(job(&state, id).state, care::CareJobState::InProgress);
        assert_eq!(job(&state, id).keeper_ids, [1, 2]);
        assert!(
            state
                .keepers
                .iter()
                .all(|k| k.activity == care::KeeperActivity::ToGate { job_id: id })
        );

        // Keeper #1 gets there first and waits inside the gate for keeper #2.
        let (inside, outside) = state.service_gate_tiles(&state.habitats[0]).unwrap();
        state.keepers[0].x = inside.x;
        state.keepers[0].y = inside.y;
        state.keepers[0].exit = Some(outside);
        state.tick(3);
        assert_eq!(
            state.keepers[0].activity,
            care::KeeperActivity::AwaitingCrew { job_id: id }
        );
        let view = state.keeper_view(&state.keepers[0]);
        assert_eq!(
            view.status,
            format!("Waiting inside habitat #{habitat_id}'s gate for keeper #2 before inspection")
        );

        let mut worked_together = false;
        for _ in 0..120 {
            state.tick(1);
            let working: Vec<bool> = state
                .keepers
                .iter()
                .map(|k| matches!(k.activity, care::KeeperActivity::Working { .. }))
                .collect();
            // Nobody works alone, and both finish the same minute.
            assert!(working[0] == working[1], "{:?}", state.keepers);
            if working[0] {
                worked_together = true;
                let handler = state.keeper_view(&state.keepers[1]).status;
                assert!(
                    handler.starts_with(&format!(
                        "Managing the animals in habitat #{habitat_id} while keeper #1 does inspection"
                    )),
                    "{handler}"
                );
            }
            if job(&state, id).state == care::CareJobState::Completed {
                break;
            }
        }
        assert!(worked_together);
        assert_eq!(job(&state, id).state, care::CareJobState::Completed);
        assert_eq!(
            state
                .care_jobs
                .iter()
                .filter(|job| job.habitat_id == habitat_id)
                .count(),
            1
        );
        assert!(state.keepers.iter().all(|k| k.activity.is_free()));
    }

    #[test]
    fn a_crew_feeding_takes_one_crate_and_credits_one_delivery() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        state.hire_zoned_keeper();
        assert!(state.upgrade_fence(habitat_id, "steel_mesh").ok);
        assert!(state.adopt(habitat_id, "elephant").ok);
        state.habitats[0].food = 40;
        let crates = state.feed_crates;
        state.tick(1);
        let feeding = feeding_job(&state).id;
        assert_eq!(job(&state, feeding).keeper_ids.len(), 2);
        assert_eq!(state.feed_crates, crates - 1);
        state.tick(60);
        assert_eq!(job(&state, feeding).state, care::CareJobState::Completed);
        assert_eq!(state.feed_crates, crates - 1);
        let deliveries: u32 = state.keepers.iter().map(|k| k.deliveries_completed).sum();
        assert_eq!(deliveries, 1);
        assert_eq!(state.keepers[0].deliveries_completed, 1);
    }

    #[test]
    fn a_blocked_crew_member_holds_both_reservations_until_the_path_returns() {
        let (mut state, _, id) = crew_park();
        assert!(state.bulldoze(2, ENTRANCE_Y).ok);
        let path_free = |state: &GameState| state.keeper_route(&state.keepers[1]).is_some();
        // Dispatch needs a route, so nothing is reserved while the path is gone.
        state.tick(1);
        assert_eq!(job(&state, id).state, care::CareJobState::Due);
        assert!(state.place_path(2, ENTRANCE_Y).ok);
        state.tick(1);
        assert_eq!(job(&state, id).keeper_ids, [1, 2]);

        // The path goes again mid-walk: both keepers keep the job, neither works alone.
        assert!(state.bulldoze(2, ENTRANCE_Y).ok);
        state.tick(30);
        assert!(!path_free(&state));
        assert_eq!(job(&state, id).state, care::CareJobState::InProgress);
        assert_eq!(job(&state, id).keeper_ids, [1, 2]);
        assert!(
            state
                .keepers
                .iter()
                .all(|k| !matches!(k.activity, care::KeeperActivity::Working { .. }))
        );
        assert!(state.place_path(2, ENTRANCE_Y).ok);
        state.tick(90);
        assert_eq!(job(&state, id).state, care::CareJobState::Completed);
    }

    #[test]
    fn bulldozing_a_dangerous_habitat_releases_its_whole_crew() {
        let (mut state, _, id) = crew_park();
        state.tick(4);
        assert_eq!(job(&state, id).keeper_ids, [1, 2]);
        assert!(state.bulldoze(3, 8).ok);
        assert!(state.care_jobs.iter().all(|job| job.id != id));
        assert!(
            state
                .keepers
                .iter()
                .all(|k| k.activity == care::KeeperActivity::AtDepot)
        );
    }

    #[test]
    fn one_large_advance_matches_many_small_advances() {
        let mut large = two_habitat_park();
        let mut small = two_habitat_park();
        assert!(large.set_care_task(2, "inspection", 30, 60, 1).ok);
        assert!(small.set_care_task(2, "inspection", 30, 60, 1).ok);
        large.tick(3 * 24 * 60 + 17);
        for _ in 0..(3 * 24 * 60 + 17) {
            small.tick(1);
        }
        assert_eq!(large.care_jobs, small.care_jobs);
        assert_eq!(large.next_care_job_id, small.next_care_job_id);
        assert_eq!(
            serde_json::to_string(&large.snapshot()).unwrap(),
            serde_json::to_string(&small.snapshot()).unwrap()
        );
        // Resolved history stays bounded to one day.
        let now = large.absolute_minute();
        assert!(
            large
                .care_jobs
                .iter()
                .all(|job| job.due_minute + 24 * 60 + 60 > now)
        );
    }

    #[test]
    fn bulldozing_habitat_releases_its_keeper() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        state.hire_zoned_keeper();
        let job_id = push_inspection(&mut state, habitat_id, 3, 0);
        state.tick(1);
        assert_eq!(job(&state, job_id).keeper_ids, [1]);

        assert!(state.bulldoze(3, 8).ok);
        assert!(state.habitats.is_empty());
        assert!(state.care_jobs.is_empty());
        assert_eq!(state.keepers[0].activity, care::KeeperActivity::AtDepot);
    }

    /// Rating of a park with no habitats, no guests and no litter.
    const EMPTY_CLEAN_PARK_RATING: u32 = 250 + 50 * 2 + 60;

    #[test]
    fn demolishing_the_last_habitat_updates_the_rating_immediately() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "zebra").ok);
        assert!(state.adopt(habitat_id, "zebra").ok);
        assert_ne!(state.snapshot().rating, EMPTY_CLEAN_PARK_RATING);

        let (cash, ledger, day, minute) = (
            state.cash_cents,
            state.finance_today,
            state.day,
            state.minute_of_day,
        );
        assert!(state.bulldoze(3, 8).ok);

        assert!(state.habitats.is_empty());
        assert_eq!(state.snapshot().rating, EMPTY_CLEAN_PARK_RATING);
        assert_eq!(state.cash_cents, cash);
        assert_eq!(state.finance_today, ledger);
        assert_eq!((state.day, state.minute_of_day), (day, minute));
    }

    #[test]
    fn demolition_that_removes_guests_updates_the_rating_immediately() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "zebra").ok);
        let mut ticks = 0;
        while !state
            .guests
            .iter()
            .any(|guest| guest.target_habitat == habitat_id)
        {
            state.tick(1);
            ticks += 1;
            assert!(ticks < 24 * 60, "no guest targeted the habitat");
        }
        state
            .guests
            .retain(|guest| guest.target_habitat == habitat_id);
        state.litter.clear();

        assert!(state.bulldoze(3, 8).ok);

        assert!(state.guests.is_empty());
        assert_eq!(state.snapshot().rating, EMPTY_CLEAN_PARK_RATING);
    }

    #[test]
    fn demolishing_a_littered_path_updates_the_rating_immediately() {
        let mut state = GameState::default();
        assert_eq!(state.snapshot().rating, EMPTY_CLEAN_PARK_RATING);
        assert!(state.add_litter(Position {
            x: 4,
            y: ENTRANCE_Y,
        }));
        assert!(state.snapshot().rating < EMPTY_CLEAN_PARK_RATING);

        assert!(state.bulldoze(4, ENTRANCE_Y).ok);

        assert!(state.litter.is_empty());
        assert_eq!(state.snapshot().rating, EMPTY_CLEAN_PARK_RATING);
    }

    #[test]
    fn keeper_wages_are_charged_by_hourly_upkeep() {
        let mut state = GameState::default();
        state.hire_zoned_keeper();
        let before = state.cash_cents;

        state.charge_upkeep();

        assert_eq!(state.cash_cents, before - KEEPER_HOURLY_WAGE);
        assert_eq!(
            state.finance_today.keeper_wages_expense_cents,
            KEEPER_HOURLY_WAGE
        );
    }

    #[test]
    fn finance_ledger_reconciles_current_economy_categories() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;

        assert!(state.buy_animal_feed().ok);
        state.hire_zoned_keeper();
        assert!(state.adopt(habitat_id, "capybara").ok);

        let care_before = state.finance_today.habitat_care_expense_cents;
        assert!(state.refill_water(habitat_id).ok);
        assert_eq!(state.finance_today.habitat_care_expense_cents, care_before);

        state.habitats[0].water = 40;
        assert!(state.refill_water(habitat_id).ok);
        state.try_spawn_guest();
        assert_eq!(state.guests.len(), 1);
        state.charge_upkeep();

        let ledger = state.finance_today;
        assert_eq!(ledger.admissions_income_cents, ADMISSION_PRICE);
        assert_eq!(
            ledger.construction_expense_cents,
            HABITAT_BASE_COST + 12 * HABITAT_TILE_COST + 14 * FENCE_SEGMENT_COST
        );
        assert_eq!(
            ledger.animal_purchase_expense_cents,
            Species::parse("capybara").unwrap().purchase_cost()
        );
        assert_eq!(ledger.habitat_care_expense_cents, WATER_REFILL_COST);
        assert_eq!(ledger.animal_feed_expense_cents, FEED_BATCH_COST);
        assert_eq!(ledger.keeper_hiring_expense_cents, KEEPER_HIRE_COST);
        assert_eq!(ledger.janitor_hiring_expense_cents, 0);
        assert_eq!(ledger.mechanic_hiring_expense_cents, 0);
        assert_eq!(ledger.maintenance_repair_expense_cents, 0);
        assert!(ledger.park_upkeep_expense_cents > 0);
        assert_eq!(ledger.keeper_wages_expense_cents, KEEPER_HOURLY_WAGE);
        assert_eq!(ledger.janitor_wages_expense_cents, 0);
        assert_eq!(ledger.mechanic_wages_expense_cents, 0);
        assert_eq!(
            ledger.expense_total_cents(),
            ledger.construction_expense_cents
                + ledger.animal_purchase_expense_cents
                + ledger.habitat_care_expense_cents
                + ledger.animal_feed_expense_cents
                + ledger.keeper_hiring_expense_cents
                + ledger.janitor_hiring_expense_cents
                + ledger.mechanic_hiring_expense_cents
                + ledger.maintenance_repair_expense_cents
                + ledger.park_upkeep_expense_cents
                + ledger.keeper_wages_expense_cents
                + ledger.janitor_wages_expense_cents
                + ledger.mechanic_wages_expense_cents
        );
        assert_eq!(
            ledger.profit_cents(),
            ledger.income_total_cents() - ledger.expense_total_cents()
        );
    }

    #[test]
    fn finance_day_rollover_preserves_previous_day_and_resets_current_day() {
        let mut state = GameState::default();
        state
            .finance_today
            .record_income(IncomeCategory::Admissions, 1_200);
        state
            .finance_today
            .record_expense(ExpenseCategory::Construction, 1_000);
        state.minute_of_day = 24 * 60 - 1;

        state.tick(1);

        assert_eq!(state.day, 2);
        assert_eq!(state.finance_today, FinanceLedger::default());
        let previous = state.finance_previous.expect("day one ledger retained");
        assert_eq!(previous.admissions_income_cents, 1_200);
        assert_eq!(previous.construction_expense_cents, 1_000);
        assert_eq!(previous.profit_cents(), 200);

        let view = FinanceView::new(
            ADMISSION_PRICE,
            state.day,
            state.finance_today,
            state.finance_previous,
        );
        assert_eq!(view.current_day.profit_cents, 0);
        assert_eq!(view.previous_day.expect("previous day view").day, 1);
        assert_eq!(view.profit_change_cents, Some(-200));
        assert_eq!(view.profit_trend, "down");
    }

    #[test]
    fn simulation_remains_deterministic() {
        let mut first = GameState::default();
        let mut second = GameState::default();

        for state in [&mut first, &mut second] {
            assert!(state.place_habitat_rect(3, 8, 7, 11).ok);
            let habitat_id = state.habitats[0].id;
            staff_habitat(state, habitat_id);
            assert!(state.adopt(habitat_id, "penguin").ok);
            assert!(state.adopt(habitat_id, "penguin").ok);
            state.tick(120);
        }

        assert_eq!(
            serde_json::to_string(&first.snapshot()).unwrap(),
            serde_json::to_string(&second.snapshot()).unwrap()
        );
    }
    mod economic_boundary_acceptance {
        include!("economic_boundary_acceptance_tests.rs");
    }
    mod circulation {
        include!("circulation_tests.rs");
    }
}
