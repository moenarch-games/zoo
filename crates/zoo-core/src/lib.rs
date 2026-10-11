use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use wasm_bindgen::prelude::*;

const WIDTH: u32 = 20;
const HEIGHT: u32 = 14;
const ENTRANCE_X: u32 = 0;
const ENTRANCE_Y: u32 = 7;
const PATH_COST: i64 = 1_000;
const LEGACY_HABITAT_WIDTH: u32 = 4;
const LEGACY_HABITAT_HEIGHT: u32 = 3;
const HABITAT_BASE_COST: i64 = 10_000;
const HABITAT_TILE_COST: i64 = 3_000;
const FENCE_SEGMENT_COST: i64 = 1_500;
const MIN_HABITAT_DIMENSION: u32 = 3;
const MAX_HABITAT_AREA: u64 = 100;
const ADMISSION_PRICE: i64 = 1_200;
const WATER_REFILL_COST: i64 = 800;
const CLEAN_HABITAT_COST: i64 = 2_000;
const SHELTER_COST: i64 = 12_000;
const CARE_DECAY_INTERVAL_MINUTES: u32 = 15;
const FOOD_STAND_BUILD_COST: i64 = 18_000;
const DRINK_STAND_BUILD_COST: i64 = 14_000;
const FOOD_PRICE: i64 = 500;
const DRINK_PRICE: i64 = 350;
const FOOD_BUY_THRESHOLD: u32 = 20;
const DRINK_BUY_THRESHOLD: u32 = 20;
const ANIMAL_CARE_DEPOT_X: u32 = 2;
const ANIMAL_CARE_DEPOT_Y: u32 = ENTRANCE_Y - 1;
const FEED_BATCH_COST: i64 = 6_000;
const FEED_BATCH_CRATES: u32 = 10;
const KEEPER_HIRE_COST: i64 = 25_000;
const KEEPER_HOURLY_WAGE: i64 = 1_200;
const JANITOR_HIRE_COST: i64 = 18_000;
const JANITOR_HOURLY_WAGE: i64 = 900;
const MECHANIC_HIRE_COST: i64 = 22_000;
const MECHANIC_HOURLY_WAGE: i64 = 1_000;
const MAINTENANCE_REPAIR_COST: i64 = 2_500;
const MAINTENANCE_DECAY_INTERVAL_MINUTES: u32 = 30;
const MAINTENANCE_WEAR_PER_INTERVAL: u32 = 8;
const MAINTENANCE_WEAR_PER_SALE: u32 = 2;
const MAINTENANCE_SERVICE_THRESHOLD: u32 = 60;
const MAINTENANCE_FAILURE_THRESHOLD: u32 = 20;
const FEED_DELIVERY_INTERVAL_MINUTES: u32 = 60;
const FEED_DELIVERY_RETRY_MINUTES: u32 = 15;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TileKind {
    Grass,
    Path,
    Entrance,
    Habitat(u32),
    Concession(u32),
    AnimalCareDepot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Position {
    x: u32,
    y: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

    fn key(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Drink => "drink",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Food => "Food stand",
            Self::Drink => "Drink stand",
        }
    }

    fn build_cost(self) -> i64 {
        match self {
            Self::Food => FOOD_STAND_BUILD_COST,
            Self::Drink => DRINK_STAND_BUILD_COST,
        }
    }

    fn price_cents(self) -> i64 {
        match self {
            Self::Food => FOOD_PRICE,
            Self::Drink => DRINK_PRICE,
        }
    }
}

#[derive(Clone, Debug)]
struct Concession {
    id: u32,
    x: u32,
    y: u32,
    kind: ConcessionKind,
    sales_today: u32,
    total_sales: u32,
    total_revenue_cents: i64,
    condition: u32,
}

#[derive(Clone, Debug)]
struct Keeper {
    id: u32,
    assigned_habitat_id: Option<u32>,
    deliveries_completed: u32,
}

#[derive(Clone, Debug)]
struct Janitor {
    id: u32,
    x: u32,
    y: u32,
    target_litter_id: Option<u32>,
    tasks_completed: u32,
}

#[derive(Clone, Debug)]
struct LitterTask {
    id: u32,
    x: u32,
    y: u32,
    created_minute: u64,
    assigned_janitor_id: Option<u32>,
}

#[derive(Clone, Debug)]
struct Mechanic {
    id: u32,
    x: u32,
    y: u32,
    target_maintenance_id: Option<u32>,
    repairs_completed: u32,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
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
    fn record_income(&mut self, category: IncomeCategory, cents: i64) {
        match category {
            IncomeCategory::Admissions => self.admissions_income_cents += cents,
            IncomeCategory::Concessions => self.concession_income_cents += cents,
        }
    }

    fn record_expense(&mut self, category: ExpenseCategory, cents: i64) {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

    fn for_dimensions(width: u32, height: u32) -> Self {
        if height > width {
            Self::Vertical
        } else {
            Self::Horizontal
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FenceSide {
    North,
    East,
    South,
    West,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Species {
    Capybara,
    Flamingo,
    Zebra,
    Giraffe,
    Elephant,
    Penguin,
}

const ALL_SPECIES: [Species; 6] = [
    Species::Capybara,
    Species::Flamingo,
    Species::Zebra,
    Species::Giraffe,
    Species::Elephant,
    Species::Penguin,
];

impl Species {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "capybara" => Some(Self::Capybara),
            "flamingo" => Some(Self::Flamingo),
            "zebra" => Some(Self::Zebra),
            "giraffe" => Some(Self::Giraffe),
            "elephant" => Some(Self::Elephant),
            "penguin" => Some(Self::Penguin),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Capybara => "capybara",
            Self::Flamingo => "flamingo",
            Self::Zebra => "zebra",
            Self::Giraffe => "giraffe",
            Self::Elephant => "elephant",
            Self::Penguin => "penguin",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Capybara => "Capybara",
            Self::Flamingo => "Flamingo",
            Self::Zebra => "Zebra",
            Self::Giraffe => "Giraffe",
            Self::Elephant => "Elephant",
            Self::Penguin => "Penguin",
        }
    }

    fn purchase_cost(self) -> i64 {
        match self {
            Self::Capybara => 25_000,
            Self::Flamingo => 18_000,
            Self::Zebra => 40_000,
            Self::Giraffe => 65_000,
            Self::Elephant => 90_000,
            Self::Penguin => 30_000,
        }
    }

    fn appeal(self) -> u32 {
        match self {
            Self::Capybara => 95,
            Self::Flamingo => 80,
            Self::Zebra => 125,
            Self::Giraffe => 160,
            Self::Elephant => 190,
            Self::Penguin => 110,
        }
    }

    fn welfare_profile(self) -> WelfareProfile {
        match self {
            Self::Capybara => WelfareProfile {
                minimum_social_group: 2,
                space_per_animal: 4,
            },
            Self::Flamingo => WelfareProfile {
                minimum_social_group: 3,
                space_per_animal: 2,
            },
            Self::Zebra => WelfareProfile {
                minimum_social_group: 3,
                space_per_animal: 5,
            },
            Self::Giraffe => WelfareProfile {
                minimum_social_group: 2,
                space_per_animal: 8,
            },
            Self::Elephant => WelfareProfile {
                minimum_social_group: 2,
                space_per_animal: 12,
            },
            Self::Penguin => WelfareProfile {
                minimum_social_group: 4,
                space_per_animal: 3,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Habitat {
    id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    orientation: HabitatOrientation,
    species: Option<Species>,
    animals: u32,
    welfare: u32,
    food: u32,
    water: u32,
    cleanliness: u32,
    has_shelter: bool,
    keeper_id: Option<u32>,
    next_feed_delivery_minute: Option<u64>,
}

impl Habitat {
    fn area(&self) -> u32 {
        self.width.saturating_mul(self.height)
    }

    fn capacity(&self) -> u32 {
        (self.area() / 3).clamp(2, 12)
    }

    fn fence_length(&self) -> u32 {
        self.width.saturating_add(self.height).saturating_mul(2)
    }

    fn fence_segments(&self) -> Vec<FenceSegment> {
        fence_segments(self.x, self.y, self.width, self.height)
    }

    fn appeal(&self) -> u32 {
        self.species.map_or(0, |species| {
            species.appeal().saturating_mul(self.animals.max(1))
        })
    }

    fn social_score(&self) -> u32 {
        let Some(species) = self.species else {
            return 100;
        };
        if self.animals == 0 {
            return 100;
        }

        let minimum_group = species.welfare_profile().minimum_social_group.max(1);
        self.animals
            .saturating_mul(100)
            .checked_div(minimum_group)
            .unwrap_or(100)
            .min(100)
    }

    fn space_score(&self) -> u32 {
        let Some(species) = self.species else {
            return 100;
        };
        if self.animals == 0 {
            return 100;
        }

        let available_space = self.area();
        let required_space = self
            .animals
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
        if self.animals == 0 { 100 } else { self.food }
    }

    fn water_score(&self) -> u32 {
        if self.animals == 0 { 100 } else { self.water }
    }

    fn cleanliness_score(&self) -> u32 {
        if self.animals == 0 {
            100
        } else {
            self.cleanliness
        }
    }

    fn shelter_score(&self) -> u32 {
        if self.animals == 0 || self.has_shelter {
            100
        } else {
            40
        }
    }

    fn welfare_target(&self) -> u32 {
        let social = self.social_score();
        let space = self.space_score();
        let food = self.food_score();
        let water = self.water_score();
        let cleanliness = self.cleanliness_score();
        let shelter = self.shelter_score();
        (social.saturating_mul(2) + space.saturating_mul(2) + food + water + cleanliness + shelter)
            / 8
    }

    fn welfare_status(&self) -> String {
        let Some(species) = self.species else {
            return "Ready for animals".to_owned();
        };
        let social = self.social_score();
        let space = self.space_score();
        let minimum_group = species.welfare_profile().minimum_social_group;

        match (social < 100, space < 100) {
            (true, true) => "Social group is too small and habitat space is crowded".to_owned(),
            (true, false) => format!("Social group needs at least {minimum_group} animals"),
            (false, true) => "Habitat space is crowded for this group".to_owned(),
            (false, false) => self.care_status(),
        }
    }

    fn care_status(&self) -> String {
        if self.keeper_id.is_none() {
            return "No keeper is scheduled for food deliveries".to_owned();
        }
        if self.animals == 0 {
            return "Keeper scheduled; care supplies are ready".to_owned();
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
        if !self.has_shelter {
            return "Animals need basic shelter".to_owned();
        }
        "Food, water, shelter, and cleanliness are healthy".to_owned()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum GuestState {
    Arriving,
    WalkingToHabitat,
    Viewing,
    WalkingToExit,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug, Serialize)]
struct PlacementEvaluation {
    ok: bool,
    message: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    orientation: HabitatOrientation,
    cost_cents: i64,
    occupied_tiles: Vec<Position>,
    fence_segments: Vec<FenceSegment>,
}

impl PlacementEvaluation {
    fn valid(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        occupied_tiles: Vec<Position>,
        fence_segments: Vec<FenceSegment>,
    ) -> Self {
        Self {
            ok: true,
            message: "Fence loop can be built here".to_owned(),
            x,
            y,
            width,
            height,
            orientation: HabitatOrientation::for_dimensions(width, height),
            cost_cents: habitat_cost(width, height),
            occupied_tiles,
            fence_segments,
        }
    }

    /// A rejection before the candidate is known to be a bounded rectangle inside the park, so
    /// no geometry is materialized for it.
    fn invalid(message: impl Into<String>, x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            ok: false,
            message: message.into(),
            x,
            y,
            width,
            height,
            orientation: HabitatOrientation::for_dimensions(width, height),
            cost_cents: habitat_cost(width, height),
            occupied_tiles: Vec::new(),
            fence_segments: Vec::new(),
        }
    }

    /// A rejection of an in-park, area-bounded candidate. It keeps the candidate footprint and
    /// fence so the preview can show where the rejected loop would have gone.
    fn rejected(
        message: impl Into<String>,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        occupied_tiles: Vec<Position>,
    ) -> Self {
        Self {
            ok: false,
            message: message.into(),
            fence_segments: fence_segments(x, y, width, height),
            occupied_tiles,
            ..Self::invalid("", x, y, width, height)
        }
    }
}

fn habitat_cost(width: u32, height: u32) -> i64 {
    let area = i64::from(width).saturating_mul(i64::from(height));
    let perimeter = i64::from(width.saturating_add(height).saturating_mul(2));
    HABITAT_BASE_COST
        .saturating_add(area.saturating_mul(HABITAT_TILE_COST))
        .saturating_add(perimeter.saturating_mul(FENCE_SEGMENT_COST))
}

fn normalized_rect(ax: u32, ay: u32, bx: u32, by: u32) -> (u32, u32, u32, u32) {
    let x = ax.min(bx);
    let y = ay.min(by);
    let width = ax.abs_diff(bx).saturating_add(1);
    let height = ay.abs_diff(by).saturating_add(1);
    (x, y, width, height)
}

fn fence_segments(x: u32, y: u32, width: u32, height: u32) -> Vec<FenceSegment> {
    let mut segments = Vec::with_capacity(
        usize::try_from(width.saturating_add(height).saturating_mul(2)).unwrap_or(0),
    );
    let right = x + width - 1;
    let bottom = y + height - 1;

    for tile_x in x..=right {
        segments.push(FenceSegment {
            x: tile_x,
            y,
            side: FenceSide::North,
        });
        segments.push(FenceSegment {
            x: tile_x,
            y: bottom,
            side: FenceSide::South,
        });
    }
    for tile_y in y..=bottom {
        segments.push(FenceSegment {
            x,
            y: tile_y,
            side: FenceSide::West,
        });
        segments.push(FenceSegment {
            x: right,
            y: tile_y,
            side: FenceSide::East,
        });
    }
    segments
}

#[derive(Clone, Debug)]
struct GameState {
    width: u32,
    height: u32,
    tiles: Vec<TileKind>,
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
    spawn_accumulator: u32,
    upkeep_accumulator: u32,
    movement_accumulator: u32,
    finance_today: FinanceLedger,
    finance_previous: Option<FinanceLedger>,
}

impl Default for GameState {
    fn default() -> Self {
        let mut state = Self {
            width: WIDTH,
            height: HEIGHT,
            tiles: vec![TileKind::Grass; (WIDTH * HEIGHT) as usize],
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
            spawn_accumulator: 0,
            upkeep_accumulator: 0,
            movement_accumulator: 0,
            finance_today: FinanceLedger::default(),
            finance_previous: None,
        };

        state.set_tile(ENTRANCE_X, ENTRANCE_Y, TileKind::Entrance);
        for x in 1..=4 {
            state.set_tile(x, ENTRANCE_Y, TileKind::Path);
        }
        state.set_tile(
            ANIMAL_CARE_DEPOT_X,
            ANIMAL_CARE_DEPOT_Y,
            TileKind::AnimalCareDepot,
        );
        state
    }
}

impl GameState {
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
        self.cash_cents -= cents;
        self.finance_today.record_expense(category, cents);
        Ok(())
    }

    fn earn(&mut self, cents: i64, category: IncomeCategory) {
        self.cash_cents += cents;
        self.finance_today.record_income(category, cents);
    }

    fn place_path(&mut self, x: u32, y: u32) -> ActionResult {
        match self.tile(x, y) {
            None => ActionResult::error("That tile is outside the park"),
            Some(TileKind::Path) => ActionResult::ok("Path already exists"),
            Some(TileKind::Entrance) => ActionResult::ok("The entrance already acts as a path"),
            Some(TileKind::Habitat(_)) => ActionResult::error("A habitat occupies that tile"),
            Some(TileKind::Concession(_)) => {
                ActionResult::error("A concession stand occupies that tile")
            }
            Some(TileKind::AnimalCareDepot) => {
                ActionResult::error("The central animal-care depot occupies that tile")
            }
            Some(TileKind::Grass) => match self.spend(PATH_COST, ExpenseCategory::Construction) {
                Ok(()) => {
                    self.set_tile(x, y, TileKind::Path);
                    self.refresh_guest_routes();
                    ActionResult::ok("Path built")
                }
                Err(message) => ActionResult::error(message),
            },
        }
    }

    fn place_concession(&mut self, x: u32, y: u32, kind_name: &str) -> ActionResult {
        let Some(kind) = ConcessionKind::parse(kind_name) else {
            return ActionResult::error("Unknown concession type");
        };

        match self.tile(x, y) {
            None => return ActionResult::error("That tile is outside the park"),
            Some(TileKind::Concession(id)) => {
                let Some(existing) = self.concessions.iter().find(|stand| stand.id == id) else {
                    return ActionResult::error("The concession tile is inconsistent");
                };
                if existing.kind == kind {
                    return ActionResult::ok(format!("{} already stands here", kind.label()));
                }
                return ActionResult::error(
                    "A different concession stand already occupies that tile",
                );
            }
            Some(TileKind::Path) | Some(TileKind::Entrance) => {
                return ActionResult::error("Build the stand on grass beside the path");
            }
            Some(TileKind::Habitat(_)) => {
                return ActionResult::error("A habitat occupies that tile");
            }
            Some(TileKind::AnimalCareDepot) => {
                return ActionResult::error("The central animal-care depot occupies that tile");
            }
            Some(TileKind::Grass) => {}
        }

        let position = Position { x, y };
        let touches_path = self
            .neighbors(position)
            .into_iter()
            .any(|neighbor| self.is_walkable(neighbor));
        if !touches_path {
            return ActionResult::error("Concession stands must touch a guest path");
        }

        if let Err(message) = self.spend(kind.build_cost(), ExpenseCategory::Construction) {
            return ActionResult::error(message);
        }

        let id = self.next_concession_id;
        self.next_concession_id += 1;
        self.set_tile(x, y, TileKind::Concession(id));
        self.concessions.push(Concession {
            id,
            x,
            y,
            kind,
            sales_today: 0,
            total_sales: 0,
            total_revenue_cents: 0,
            condition: 100,
        });

        ActionResult::ok(format!("{} #{id} built beside the path", kind.label()))
    }

    fn evaluate_habitat_rect(&self, ax: u32, ay: u32, bx: u32, by: u32) -> PlacementEvaluation {
        let (x, y, width, height) = normalized_rect(ax, ay, bx, by);
        if width < MIN_HABITAT_DIMENSION || height < MIN_HABITAT_DIMENSION {
            return PlacementEvaluation::invalid(
                format!(
                    "Draw at least a {MIN_HABITAT_DIMENSION}×{MIN_HABITAT_DIMENSION} fence loop"
                ),
                x,
                y,
                width,
                height,
            );
        }

        let area = u64::from(width).saturating_mul(u64::from(height));
        if area > MAX_HABITAT_AREA {
            return PlacementEvaluation::invalid(
                format!(
                    "This fence encloses {area} tiles; the current limit is {MAX_HABITAT_AREA}"
                ),
                x,
                y,
                width,
                height,
            );
        }

        let in_bounds = x
            .checked_add(width)
            .is_some_and(|right| right <= self.width)
            && y.checked_add(height)
                .is_some_and(|bottom| bottom <= self.height);
        if !in_bounds {
            return PlacementEvaluation::invalid(
                "The fence would extend outside the park",
                x,
                y,
                width,
                height,
            );
        }

        let mut occupied_tiles = Vec::with_capacity((width * height) as usize);
        for tile_y in y..y + height {
            for tile_x in x..x + width {
                occupied_tiles.push(Position {
                    x: tile_x,
                    y: tile_y,
                });
            }
        }

        if occupied_tiles
            .iter()
            .any(|tile| self.tile(tile.x, tile.y) != Some(TileKind::Grass))
        {
            return PlacementEvaluation::rejected(
                "The enclosed area must be clear grass",
                x,
                y,
                width,
                height,
                occupied_tiles,
            );
        }

        let touches_path = occupied_tiles.iter().any(|tile| {
            self.neighbors(*tile).into_iter().any(|neighbor| {
                matches!(
                    self.tile(neighbor.x, neighbor.y),
                    Some(TileKind::Path | TileKind::Entrance)
                )
            })
        });
        if !touches_path {
            return PlacementEvaluation::rejected(
                "The fence needs at least one path along its outside edge",
                x,
                y,
                width,
                height,
                occupied_tiles,
            );
        }

        let cost = habitat_cost(width, height);
        if self.cash_cents < cost {
            return PlacementEvaluation::rejected(
                "Not enough cash",
                x,
                y,
                width,
                height,
                occupied_tiles,
            );
        }

        PlacementEvaluation::valid(
            x,
            y,
            width,
            height,
            occupied_tiles,
            fence_segments(x, y, width, height),
        )
    }

    fn evaluate_habitat(
        &self,
        x: u32,
        y: u32,
        orientation: HabitatOrientation,
    ) -> PlacementEvaluation {
        let (width, height) = orientation.dimensions();
        self.evaluate_habitat_rect(
            x,
            y,
            x.saturating_add(width - 1),
            y.saturating_add(height - 1),
        )
    }

    fn place_habitat_rect(&mut self, ax: u32, ay: u32, bx: u32, by: u32) -> ActionResult {
        let (x, y, width, height) = normalized_rect(ax, ay, bx, by);
        if let Some(existing) = self.habitats.iter().find(|habitat| {
            habitat.x == x && habitat.y == y && habitat.width == width && habitat.height == height
        }) {
            return ActionResult::ok(format!("Habitat #{} already uses this fence", existing.id));
        }

        let evaluation = self.evaluate_habitat_rect(ax, ay, bx, by);
        if !evaluation.ok {
            return ActionResult::error(evaluation.message);
        }
        if let Err(message) = self.spend(evaluation.cost_cents, ExpenseCategory::Construction) {
            return ActionResult::error(message);
        }

        let id = self.next_habitat_id;
        self.next_habitat_id += 1;
        for tile in &evaluation.occupied_tiles {
            self.set_tile(tile.x, tile.y, TileKind::Habitat(id));
        }
        self.habitats.push(Habitat {
            id,
            x: evaluation.x,
            y: evaluation.y,
            width: evaluation.width,
            height: evaluation.height,
            orientation: evaluation.orientation,
            species: None,
            animals: 0,
            welfare: 100,
            food: 100,
            water: 100,
            cleanliness: 100,
            has_shelter: false,
            keeper_id: None,
            next_feed_delivery_minute: None,
        });

        ActionResult::ok(format!(
            "Habitat #{id} fenced · {}×{} · {} tiles",
            evaluation.width,
            evaluation.height,
            evaluation.width.saturating_mul(evaluation.height)
        ))
    }

    fn place_habitat(&mut self, x: u32, y: u32, orientation: HabitatOrientation) -> ActionResult {
        let (width, height) = orientation.dimensions();
        self.place_habitat_rect(
            x,
            y,
            x.saturating_add(width - 1),
            y.saturating_add(height - 1),
        )
    }

    fn bulldoze(&mut self, x: u32, y: u32) -> ActionResult {
        match self.tile(x, y) {
            None => ActionResult::error("That tile is outside the park"),
            Some(TileKind::Grass) => ActionResult::ok("Nothing to demolish"),
            Some(TileKind::Entrance) => {
                ActionResult::error("The park entrance cannot be demolished")
            }
            Some(TileKind::Path) => {
                if self.guests.iter().any(|guest| guest.x == x && guest.y == y) {
                    return ActionResult::error("A guest is standing on that path tile");
                }
                if self
                    .janitors
                    .iter()
                    .any(|janitor| janitor.x == x && janitor.y == y)
                {
                    return ActionResult::error("A janitor is standing on that path tile");
                }
                if self
                    .mechanics
                    .iter()
                    .any(|mechanic| mechanic.x == x && mechanic.y == y)
                {
                    return ActionResult::error("A mechanic is standing on that path tile");
                }
                let removed_litter_ids: Vec<u32> = self
                    .litter
                    .iter()
                    .filter(|task| task.x == x && task.y == y)
                    .map(|task| task.id)
                    .collect();
                self.litter.retain(|task| task.x != x || task.y != y);
                for janitor in &mut self.janitors {
                    if janitor
                        .target_litter_id
                        .is_some_and(|id| removed_litter_ids.contains(&id))
                    {
                        janitor.target_litter_id = None;
                    }
                }
                self.set_tile(x, y, TileKind::Grass);
                self.refresh_guest_routes();
                self.release_unreachable_mechanic_assignments();
                ActionResult::ok("Path removed")
            }
            Some(TileKind::Concession(id)) => {
                self.set_tile(x, y, TileKind::Grass);
                let removed_task_ids: Vec<u32> = self
                    .maintenance
                    .iter()
                    .filter(|task| task.concession_id == id)
                    .map(|task| task.id)
                    .collect();
                self.maintenance.retain(|task| task.concession_id != id);
                for mechanic in &mut self.mechanics {
                    if mechanic
                        .target_maintenance_id
                        .is_some_and(|task_id| removed_task_ids.contains(&task_id))
                    {
                        mechanic.target_maintenance_id = None;
                    }
                }
                self.concessions.retain(|stand| stand.id != id);
                ActionResult::ok(format!("Concession stand #{id} removed"))
            }
            Some(TileKind::AnimalCareDepot) => {
                ActionResult::error("The central animal-care depot cannot be demolished")
            }
            Some(TileKind::Habitat(id)) => {
                for tile_y in 0..self.height {
                    for tile_x in 0..self.width {
                        if self.tile(tile_x, tile_y) == Some(TileKind::Habitat(id)) {
                            self.set_tile(tile_x, tile_y, TileKind::Grass);
                        }
                    }
                }
                self.habitats.retain(|habitat| habitat.id != id);
                for keeper in &mut self.keepers {
                    if keeper.assigned_habitat_id == Some(id) {
                        keeper.assigned_habitat_id = None;
                    }
                }
                self.guests.retain(|guest| guest.target_habitat != id);
                ActionResult::ok(format!("Habitat #{id} removed and its keeper released"))
            }
        }
    }

    fn adopt(&mut self, habitat_id: u32, species_name: &str) -> ActionResult {
        let Some(species) = Species::parse(species_name) else {
            return ActionResult::error("Unknown species");
        };
        let Some(index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
        else {
            return ActionResult::error("Select a habitat first");
        };

        let habitat = &self.habitats[index];
        if habitat.animals >= habitat.capacity() {
            return ActionResult::error("That habitat is at capacity");
        }
        if habitat.species.is_some_and(|current| current != species) {
            return ActionResult::error("Each habitat currently keeps one species");
        }
        if habitat.keeper_id.is_none() {
            return ActionResult::error("Schedule a keeper before adopting animals");
        }

        if let Err(message) = self.spend(species.purchase_cost(), ExpenseCategory::AnimalPurchase) {
            return ActionResult::error(message);
        }
        let habitat = &mut self.habitats[index];
        habitat.species = Some(species);
        habitat.animals += 1;
        ActionResult::ok(format!(
            "{} adopted into habitat #{habitat_id}",
            species.label()
        ))
    }

    fn absolute_minute(&self) -> u64 {
        u64::from(self.day.saturating_sub(1))
            .saturating_mul(24 * 60)
            .saturating_add(u64::from(self.minute_of_day))
    }

    fn buy_animal_feed(&mut self) -> ActionResult {
        if let Err(message) = self.spend(FEED_BATCH_COST, ExpenseCategory::AnimalFeed) {
            return ActionResult::error(message);
        }
        self.feed_crates = self.feed_crates.saturating_add(FEED_BATCH_CRATES);
        ActionResult::ok(format!(
            "Bought {FEED_BATCH_CRATES} feed crates for the animal-care depot"
        ))
    }

    fn hire_keeper(&mut self) -> ActionResult {
        if let Err(message) = self.spend(KEEPER_HIRE_COST, ExpenseCategory::KeeperHiring) {
            return ActionResult::error(message);
        }
        let id = self.next_keeper_id;
        self.next_keeper_id = self.next_keeper_id.saturating_add(1);
        self.keepers.push(Keeper {
            id,
            assigned_habitat_id: None,
            deliveries_completed: 0,
        });
        ActionResult::ok(format!("Keeper #{id} hired at the animal-care depot"))
    }

    fn depot_staff_spawn(&self) -> Option<Position> {
        let access_tiles = self.neighbors(Position {
            x: ANIMAL_CARE_DEPOT_X,
            y: ANIMAL_CARE_DEPOT_Y,
        });
        self.path_to_any(
            Position {
                x: ENTRANCE_X,
                y: ENTRANCE_Y,
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
        if let Err(message) = self.spend(JANITOR_HIRE_COST, ExpenseCategory::JanitorHiring) {
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
        if let Err(message) = self.spend(MECHANIC_HIRE_COST, ExpenseCategory::MechanicHiring) {
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

    fn schedule_keeper(&mut self, habitat_id: u32) -> ActionResult {
        let Some(habitat_index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
        else {
            return ActionResult::error("Select a habitat first");
        };
        if let Some(keeper_id) = self.habitats[habitat_index].keeper_id {
            return ActionResult::ok(format!(
                "Keeper #{keeper_id} is already scheduled for habitat #{habitat_id}"
            ));
        }
        let Some(keeper_index) = self
            .keepers
            .iter()
            .position(|keeper| keeper.assigned_habitat_id.is_none())
        else {
            return ActionResult::error(
                "No keeper is available; hire another at the animal-care depot",
            );
        };

        let keeper_id = self.keepers[keeper_index].id;
        let next_delivery = self
            .absolute_minute()
            .saturating_add(u64::from(FEED_DELIVERY_INTERVAL_MINUTES));
        self.keepers[keeper_index].assigned_habitat_id = Some(habitat_id);
        self.habitats[habitat_index].keeper_id = Some(keeper_id);
        self.habitats[habitat_index].next_feed_delivery_minute = Some(next_delivery);
        ActionResult::ok(format!(
            "Keeper #{keeper_id} scheduled for habitat #{habitat_id}"
        ))
    }

    fn feed_habitat(&mut self, habitat_id: u32) -> ActionResult {
        let Some(index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
        else {
            return ActionResult::error("Select a habitat first");
        };
        if self.habitats[index].food >= 100 {
            return ActionResult::ok("Food is already fully stocked");
        }
        let Some(keeper_id) = self.habitats[index].keeper_id else {
            return ActionResult::error("Schedule a keeper before delivering food");
        };
        if self.feed_crates == 0 {
            return ActionResult::error("The animal-care depot is out of feed crates");
        }

        self.feed_crates -= 1;
        self.habitats[index].food = 100;
        self.habitats[index].next_feed_delivery_minute = Some(
            self.absolute_minute()
                .saturating_add(u64::from(FEED_DELIVERY_INTERVAL_MINUTES)),
        );
        if let Some(keeper) = self
            .keepers
            .iter_mut()
            .find(|keeper| keeper.id == keeper_id)
        {
            keeper.deliveries_completed = keeper.deliveries_completed.saturating_add(1);
        }
        ActionResult::ok(format!(
            "Keeper #{keeper_id} delivered feed to habitat #{habitat_id}"
        ))
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
        if let Err(message) = self.spend(WATER_REFILL_COST, ExpenseCategory::HabitatCare) {
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
        if let Err(message) = self.spend(CLEAN_HABITAT_COST, ExpenseCategory::HabitatCare) {
            return ActionResult::error(message);
        }
        self.habitats[index].cleanliness = 100;
        ActionResult::ok(format!("Habitat #{habitat_id} cleaned"))
    }

    fn add_shelter(&mut self, habitat_id: u32) -> ActionResult {
        let Some(index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
        else {
            return ActionResult::error("Select a habitat first");
        };
        if self.habitats[index].has_shelter {
            return ActionResult::ok("Basic shelter is already installed");
        }
        if let Err(message) = self.spend(SHELTER_COST, ExpenseCategory::HabitatCare) {
            return ActionResult::error(message);
        }
        self.habitats[index].has_shelter = true;
        ActionResult::ok(format!("Basic shelter added to habitat #{habitat_id}"))
    }

    fn tick(&mut self, minutes: u32) {
        for _ in 0..minutes {
            self.minute_of_day += 1;
            if self.minute_of_day >= 24 * 60 {
                self.minute_of_day = 0;
                self.day += 1;
                self.finance_previous = Some(self.finance_today);
                self.finance_today = FinanceLedger::default();
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
            self.advance_keeper_deliveries();

            self.advance_animal_welfare();
            self.advance_guest_needs();

            if self.movement_accumulator >= 3 {
                self.movement_accumulator = 0;
                self.advance_guest_movement();
                self.advance_janitor_work();
                self.advance_mechanic_work();
            }

            self.advance_viewing();
        }
    }

    fn advance_habitat_care(&mut self) {
        for habitat in &mut self.habitats {
            if habitat.animals == 0 {
                continue;
            }
            habitat.food = habitat.food.saturating_sub(habitat.animals);
            habitat.water = habitat.water.saturating_sub(habitat.animals);
            let waste = habitat.animals.saturating_add(1) / 2;
            habitat.cleanliness = habitat.cleanliness.saturating_sub(waste.max(1));
        }
    }

    fn advance_keeper_deliveries(&mut self) {
        let now = self.absolute_minute();
        let due: Vec<(usize, u32)> = self
            .habitats
            .iter()
            .enumerate()
            .filter_map(|(index, habitat)| {
                let keeper_id = habitat.keeper_id?;
                let delivery_minute = habitat.next_feed_delivery_minute?;
                (delivery_minute <= now).then_some((index, keeper_id))
            })
            .collect();

        for (habitat_index, keeper_id) in due {
            let needs_feed = self.habitats[habitat_index].animals > 0
                && self.habitats[habitat_index].food < FEED_DELIVERY_THRESHOLD;
            let retry_minutes = if needs_feed && self.feed_crates == 0 {
                FEED_DELIVERY_RETRY_MINUTES
            } else {
                FEED_DELIVERY_INTERVAL_MINUTES
            };

            if needs_feed && self.feed_crates > 0 {
                self.feed_crates -= 1;
                self.habitats[habitat_index].food = 100;
                if let Some(keeper) = self
                    .keepers
                    .iter_mut()
                    .find(|keeper| keeper.id == keeper_id)
                {
                    keeper.deliveries_completed = keeper.deliveries_completed.saturating_add(1);
                }
            }

            self.habitats[habitat_index].next_feed_delivery_minute =
                Some(now.saturating_add(u64::from(retry_minutes)));
        }
    }

    fn advance_animal_welfare(&mut self) {
        for habitat in &mut self.habitats {
            if habitat.animals == 0 {
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
            x: ENTRANCE_X,
            y: ENTRANCE_Y,
        };
        let viewing_occupancy = self.viewing_occupancy();
        let mut candidates: Vec<(u32, Vec<Position>)> = self
            .habitats
            .iter()
            .filter(|habitat| habitat.animals > 0)
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

        self.earn(ADMISSION_PRICE, IncomeCategory::Admissions);
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
        self.next_guest_id += 1;
    }

    fn advance_guest_needs(&mut self) {
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
            .filter(|habitat| habitat.animals > 0)
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

    fn refresh_guest_routes(&mut self) {
        // Topology edits, not simulation ticks, retry blocked routes. Preserve valid
        // cached routes so unrelated construction does not restart guest journeys.
        let viewing_occupancy = self.viewing_occupancy();
        for index in 0..self.guests.len() {
            let guest = &self.guests[index];
            if guest.state == GuestState::Viewing {
                continue;
            }
            let still_walkable = guest
                .route
                .get(guest.route_index..)
                .is_some_and(|remaining| {
                    !remaining.is_empty()
                        && remaining.iter().all(|position| self.is_walkable(*position))
                });
            if still_walkable {
                continue;
            }
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
                    start,
                    Position {
                        x: ENTRANCE_X,
                        y: ENTRANCE_Y,
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
                if !self.is_walkable(position) {
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
            let price = kind.price_cents();
            self.earn(price, IncomeCategory::Concessions);
            {
                let stand = &mut self.concessions[concession_index];
                stand.sales_today += 1;
                stand.total_sales += 1;
                stand.total_revenue_cents += price;
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
                x: ENTRANCE_X,
                y: ENTRANCE_Y,
            };
            if let Some(route) = self.path_between(start, exit) {
                let guest = &mut self.guests[index];
                guest.state = GuestState::WalkingToExit;
                guest.route = route;
                guest.route_index = 0;
            }
        }
    }

    fn add_litter(&mut self, position: Position) -> bool {
        if !self.is_walkable(position)
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
            let reachable =
                target.is_some_and(|target| self.path_between(janitor_position, target).is_some());
            if reachable {
                continue;
            }
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
            let Some(route) = self.path_between(current, target) else {
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

    fn concession_service_tiles(&self, concession_id: u32) -> Vec<Position> {
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
        .filter(|position| self.is_walkable(*position))
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
                self.path_between(start, target)
                    .map(|route| (route.len(), target))
            })
            .min_by_key(|(route_len, target)| (*route_len, target.y, target.x))
            .map(|(_, target)| target)
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
            .spend(MAINTENANCE_REPAIR_COST, ExpenseCategory::MaintenanceRepair)
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
            let Some(route) = self.path_between(current, target) else {
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
        let animal_count: i64 = self
            .habitats
            .iter()
            .map(|habitat| i64::from(habitat.animals))
            .sum();
        let fence_count: i64 = self
            .habitats
            .iter()
            .map(|habitat| i64::from(habitat.fence_length()))
            .sum();
        let park_upkeep = self.habitats.len() as i64 * 250
            + animal_count * 125
            + fence_count * 8
            + self.concessions.len() as i64 * 50;
        let keeper_wages = self.keepers.len() as i64 * KEEPER_HOURLY_WAGE;
        let janitor_wages = self.janitors.len() as i64 * JANITOR_HOURLY_WAGE;
        let mechanic_wages = self.mechanics.len() as i64 * MECHANIC_HOURLY_WAGE;
        self.cash_cents -= park_upkeep + keeper_wages + janitor_wages + mechanic_wages;
        self.finance_today
            .record_expense(ExpenseCategory::ParkUpkeep, park_upkeep);
        self.finance_today
            .record_expense(ExpenseCategory::KeeperWages, keeper_wages);
        self.finance_today
            .record_expense(ExpenseCategory::JanitorWages, janitor_wages);
        self.finance_today
            .record_expense(ExpenseCategory::MechanicWages, mechanic_wages);
    }

    /// Park rating derived from the current state, so every command and tick reports a
    /// self-consistent value without a separate refresh step.
    fn rating(&self) -> u32 {
        let appeal: u32 = self.habitats.iter().map(Habitat::appeal).sum();
        let welfare = if self.habitats.iter().any(|habitat| habitat.animals > 0) {
            let total: u32 = self
                .habitats
                .iter()
                .filter(|habitat| habitat.animals > 0)
                .map(|habitat| habitat.welfare)
                .sum();
            let count = self
                .habitats
                .iter()
                .filter(|habitat| habitat.animals > 0)
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

    fn is_walkable(&self, position: Position) -> bool {
        matches!(
            self.tile(position.x, position.y),
            Some(TileKind::Path | TileKind::Entrance)
        )
    }

    fn viewing_tile_visible(
        &self,
        habitat: &Habitat,
        viewpoint: Position,
        side: FenceSide,
        target: Position,
    ) -> bool {
        let right = habitat.x.saturating_add(habitat.width.saturating_sub(1));
        let bottom = habitat.y.saturating_add(habitat.height.saturating_sub(1));
        if target.x < habitat.x || target.x > right || target.y < habitat.y || target.y > bottom {
            return false;
        }

        match side {
            FenceSide::North => {
                target.y.saturating_sub(habitat.y) < VIEWING_DEPTH_TILES
                    && target.x.abs_diff(viewpoint.x) <= VIEWING_HALF_WIDTH_TILES
            }
            FenceSide::South => {
                bottom.saturating_sub(target.y) < VIEWING_DEPTH_TILES
                    && target.x.abs_diff(viewpoint.x) <= VIEWING_HALF_WIDTH_TILES
            }
            FenceSide::West => {
                target.x.saturating_sub(habitat.x) < VIEWING_DEPTH_TILES
                    && target.y.abs_diff(viewpoint.y) <= VIEWING_HALF_WIDTH_TILES
            }
            FenceSide::East => {
                right.saturating_sub(target.x) < VIEWING_DEPTH_TILES
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
        let mut positions = Vec::new();
        for y in habitat.y..habitat.y.saturating_add(habitat.height) {
            for x in habitat.x..habitat.x.saturating_add(habitat.width) {
                let position = Position { x, y };
                if self.viewing_tile_visible(habitat, viewpoint, side, position) {
                    positions.push(position);
                }
            }
        }
        positions
    }

    fn animal_position(&self, habitat: &Habitat, slot: u32) -> Position {
        let inner_width = habitat.width.saturating_sub(2).max(1);
        let inner_height = habitat.height.saturating_sub(2).max(1);
        let inner_area = inner_width.saturating_mul(inner_height).max(1);
        let time_step = self.minute_of_day / 2;
        let index = time_step
            .saturating_add(slot.saturating_mul(3))
            .saturating_add(habitat.id.saturating_mul(5))
            % inner_area;
        let local_x = index % inner_width;
        let local_y = index / inner_width;
        Position {
            x: habitat.x.saturating_add(1).saturating_add(local_x),
            y: habitat.y.saturating_add(1).saturating_add(local_y),
        }
    }

    fn visible_animals_from(&self, habitat: &Habitat, viewpoint: Position, side: FenceSide) -> u32 {
        (0..habitat.animals)
            .filter(|slot| {
                self.viewing_tile_visible(
                    habitat,
                    viewpoint,
                    side,
                    self.animal_position(habitat, *slot),
                )
            })
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    fn viewing_side_for_position(
        &self,
        habitat: &Habitat,
        position: Position,
    ) -> Option<FenceSide> {
        let right = habitat.x.saturating_add(habitat.width.saturating_sub(1));
        let bottom = habitat.y.saturating_add(habitat.height.saturating_sub(1));

        if habitat.y > 0 && position.y == habitat.y - 1 && (habitat.x..=right).contains(&position.x)
        {
            return Some(FenceSide::North);
        }
        if right.saturating_add(1) < self.width
            && position.x == right + 1
            && (habitat.y..=bottom).contains(&position.y)
        {
            return Some(FenceSide::East);
        }
        if bottom.saturating_add(1) < self.height
            && position.y == bottom + 1
            && (habitat.x..=right).contains(&position.x)
        {
            return Some(FenceSide::South);
        }
        if habitat.x > 0
            && position.x == habitat.x - 1
            && (habitat.y..=bottom).contains(&position.y)
        {
            return Some(FenceSide::West);
        }
        None
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
        let mut candidates = Vec::new();
        let right = habitat.x.saturating_add(habitat.width.saturating_sub(1));
        let bottom = habitat.y.saturating_add(habitat.height.saturating_sub(1));

        if habitat.y > 0 {
            for x in habitat.x..=right {
                candidates.push((
                    Position {
                        x,
                        y: habitat.y - 1,
                    },
                    FenceSide::North,
                ));
            }
        }
        if right.saturating_add(1) < self.width {
            for y in habitat.y..=bottom {
                candidates.push((Position { x: right + 1, y }, FenceSide::East));
            }
        }
        if bottom.saturating_add(1) < self.height {
            for x in habitat.x..=right {
                candidates.push((Position { x, y: bottom + 1 }, FenceSide::South));
            }
        }
        if habitat.x > 0 {
            for y in habitat.y..=bottom {
                candidates.push((
                    Position {
                        x: habitat.x - 1,
                        y,
                    },
                    FenceSide::West,
                ));
            }
        }

        candidates
            .into_iter()
            .filter(|(position, _)| self.is_walkable(*position))
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
        if !self.is_walkable(position) {
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
                self.path_between(start, spot.position)
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

    fn path_between(&self, start: Position, goal: Position) -> Option<Vec<Position>> {
        if !self.is_walkable(goal) {
            return None;
        }
        self.path_to_any(start, &[goal])
    }

    fn path_to_any(&self, start: Position, goals: &[Position]) -> Option<Vec<Position>> {
        if !self.is_walkable(start) || goals.is_empty() {
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
                if !self.is_walkable(next) || previous.contains_key(&(next.x, next.y)) {
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
        ALL_SPECIES
            .into_iter()
            .map(|species| {
                let profile = species.welfare_profile();
                SpeciesOfferView {
                    key: species.key().to_owned(),
                    label: species.label().to_owned(),
                    purchase_cost_cents: species.purchase_cost(),
                    appeal: species.appeal(),
                    minimum_social_group: profile.minimum_social_group,
                    space_per_animal: profile.space_per_animal,
                }
            })
            .collect()
    }

    fn animal_views(&self) -> Vec<AnimalView> {
        let mut animals = Vec::new();
        for habitat in &self.habitats {
            let Some(species) = habitat.species else {
                continue;
            };

            for slot in 0..habitat.animals {
                let position = self.animal_position(habitat, slot);
                animals.push(AnimalView {
                    id: habitat.id.saturating_mul(100).saturating_add(slot + 1),
                    habitat_id: habitat.id,
                    species: species.key().to_owned(),
                    x: position.x,
                    y: position.y,
                    slot,
                    animation_phase: (slot
                        .saturating_mul(17)
                        .saturating_add(habitat.id.saturating_mul(7)))
                        % 100,
                });
            }
        }
        animals
    }

    fn feeding_status(&self, habitat: &Habitat) -> String {
        let Some(keeper_id) = habitat.keeper_id else {
            return "No keeper scheduled; hire staff at the animal-care depot".to_owned();
        };
        if self.feed_crates == 0 {
            return format!("Keeper #{keeper_id} is waiting for depot feed stock");
        }
        let minutes = habitat
            .next_feed_delivery_minute
            .map(|due| due.saturating_sub(self.absolute_minute()))
            .unwrap_or(0);
        format!("Keeper #{keeper_id} scheduled · next food run in {minutes} min")
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
            if at_service_tile && self.cash_cents < MAINTENANCE_REPAIR_COST {
                return format!(
                    "Waiting for repair parts at stand #{} · ${} needed",
                    task.concession_id,
                    MAINTENANCE_REPAIR_COST / 100
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
            x: ANIMAL_CARE_DEPOT_X,
            y: ANIMAL_CARE_DEPOT_Y,
            feed_crates: self.feed_crates,
            feed_batch_crates: FEED_BATCH_CRATES,
            feed_batch_cost_cents: FEED_BATCH_COST,
            keeper_hire_cost_cents: KEEPER_HIRE_COST,
            keeper_hourly_wage_cents: KEEPER_HOURLY_WAGE,
            janitor_hire_cost_cents: JANITOR_HIRE_COST,
            janitor_hourly_wage_cents: JANITOR_HOURLY_WAGE,
            mechanic_hire_cost_cents: MECHANIC_HIRE_COST,
            mechanic_hourly_wage_cents: MECHANIC_HOURLY_WAGE,
            maintenance_repair_cost_cents: MAINTENANCE_REPAIR_COST,
            keepers: self
                .keepers
                .iter()
                .map(|keeper| KeeperView {
                    id: keeper.id,
                    assigned_habitat_id: keeper.assigned_habitat_id,
                    deliveries_completed: keeper.deliveries_completed,
                    status: keeper.assigned_habitat_id.map_or_else(
                        || "Available for assignment".to_owned(),
                        |habitat_id| format!("Scheduled for habitat #{habitat_id}"),
                    ),
                })
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
                    TileKind::Path => "path",
                    TileKind::Entrance => "entrance",
                    TileKind::Habitat(_) => "habitat",
                    TileKind::Concession(_) => "concession",
                    TileKind::AnimalCareDepot => "grass",
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
                HabitatView {
                    id: habitat.id,
                    x: habitat.x,
                    y: habitat.y,
                    width: habitat.width,
                    height: habitat.height,
                    orientation: habitat.orientation,
                    footprint_area: habitat.area(),
                    fence_length: habitat.fence_length(),
                    fence_segments: habitat.fence_segments(),
                    species: habitat.species.map(|species| species.key().to_owned()),
                    animals: habitat.animals,
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
                    welfare_status: habitat.welfare_status(),
                    food: habitat.food,
                    water: habitat.water,
                    cleanliness: habitat.cleanliness,
                    has_shelter: habitat.has_shelter,
                    keeper_id: habitat.keeper_id,
                    next_feed_delivery_in_minutes: habitat
                        .next_feed_delivery_minute
                        .map(|due| due.saturating_sub(self.absolute_minute())),
                    feeding_status: self.feeding_status(habitat),
                    care_status: habitat.care_status(),
                    appeal: habitat.appeal(),
                }
            })
            .collect();

        let concessions = self
            .concessions
            .iter()
            .map(|stand| ConcessionView {
                id: stand.id,
                x: stand.x,
                y: stand.y,
                kind: stand.kind.key().to_owned(),
                build_cost_cents: stand.kind.build_cost(),
                price_cents: stand.kind.price_cents(),
                sales_today: stand.sales_today,
                total_sales: stand.total_sales,
                total_revenue_cents: stand.total_revenue_cents,
                condition: stand.condition,
                service_state: self.concession_service_state(stand).to_owned(),
                maintenance_status: self.concession_maintenance_status(stand),
            })
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
            width: self.width,
            height: self.height,
            day: self.day,
            minute_of_day: self.minute_of_day,
            cash_cents: self.cash_cents,
            rating: self.rating(),
            guest_count: self.guests.len() as u32,
            entrance: EntranceView {
                x: ENTRANCE_X,
                y: ENTRANCE_Y,
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
            complaints: self.complaint_summary(),
            finance: FinanceView::new(self.day, self.finance_today, self.finance_previous),
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
}

#[derive(Serialize)]
struct KeeperView {
    id: u32,
    assigned_habitat_id: Option<u32>,
    deliveries_completed: u32,
    status: String,
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
    orientation: HabitatOrientation,
    footprint_area: u32,
    fence_length: u32,
    fence_segments: Vec<FenceSegment>,
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
    has_shelter: bool,
    keeper_id: Option<u32>,
    next_feed_delivery_in_minutes: Option<u64>,
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
    x: u32,
    y: u32,
    slot: u32,
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
    label: String,
    purchase_cost_cents: i64,
    appeal: u32,
    minimum_social_group: u32,
    space_per_animal: u32,
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
    fn new(day: u32, current: FinanceLedger, previous: Option<FinanceLedger>) -> Self {
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
            admission_price_cents: ADMISSION_PRICE,
            current_day,
            previous_day,
            profit_change_cents,
            profit_trend,
        }
    }
}

#[derive(Serialize)]
struct Snapshot {
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
    operations: OperationsView,
    species_catalog: Vec<SpeciesOfferView>,
    complaints: ComplaintSummary,
    finance: FinanceView,
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

#[wasm_bindgen]
pub struct ZooGame {
    state: GameState,
}

#[wasm_bindgen]
impl ZooGame {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: GameState::default(),
        }
    }

    pub fn reset(&mut self) {
        self.state = GameState::default();
    }

    pub fn snapshot_json(&self) -> String {
        serde_json::to_string(&self.state.snapshot()).expect("snapshot serialization is infallible")
    }

    pub fn place_path(&mut self, x: u32, y: u32) -> String {
        self.state.place_path(x, y).json()
    }

    pub fn place_concession(&mut self, x: u32, y: u32, kind: String) -> String {
        self.state.place_concession(x, y, &kind).json()
    }

    pub fn evaluate_habitat_rect(&self, ax: u32, ay: u32, bx: u32, by: u32) -> String {
        serde_json::to_string(&self.state.evaluate_habitat_rect(ax, ay, bx, by))
            .expect("placement evaluation serialization is infallible")
    }

    pub fn place_habitat_rect(&mut self, ax: u32, ay: u32, bx: u32, by: u32) -> String {
        self.state.place_habitat_rect(ax, ay, bx, by).json()
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
        self.state
            .place_habitat(x, y, HabitatOrientation::from_code(orientation))
            .json()
    }

    pub fn bulldoze(&mut self, x: u32, y: u32) -> String {
        self.state.bulldoze(x, y).json()
    }

    pub fn adopt(&mut self, habitat_id: u32, species: String) -> String {
        self.state.adopt(habitat_id, &species).json()
    }

    pub fn buy_animal_feed(&mut self) -> String {
        self.state.buy_animal_feed().json()
    }

    pub fn hire_keeper(&mut self) -> String {
        self.state.hire_keeper().json()
    }

    pub fn hire_janitor(&mut self) -> String {
        self.state.hire_janitor().json()
    }

    pub fn hire_mechanic(&mut self) -> String {
        self.state.hire_mechanic().json()
    }

    pub fn schedule_keeper(&mut self, habitat_id: u32) -> String {
        self.state.schedule_keeper(habitat_id).json()
    }

    pub fn feed_habitat(&mut self, habitat_id: u32) -> String {
        self.state.feed_habitat(habitat_id).json()
    }

    pub fn refill_water(&mut self, habitat_id: u32) -> String {
        self.state.refill_water(habitat_id).json()
    }

    pub fn clean_habitat(&mut self, habitat_id: u32) -> String {
        self.state.clean_habitat(habitat_id).json()
    }

    pub fn add_shelter(&mut self, habitat_id: u32) -> String {
        self.state.add_shelter(habitat_id).json()
    }

    pub fn tick(&mut self, minutes: u32) {
        self.state.tick(minutes.min(240));
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

    fn staff_habitat(state: &mut GameState, habitat_id: u32) {
        assert!(state.buy_animal_feed().ok);
        assert!(state.hire_keeper().ok);
        assert!(state.schedule_keeper(habitat_id).ok);
    }

    fn test_habitat(x: u32, y: u32, width: u32, height: u32) -> Habitat {
        Habitat {
            id: 1,
            x,
            y,
            width,
            height,
            orientation: HabitatOrientation::for_dimensions(width, height),
            species: Some(Species::parse("capybara").expect("default catalog capybara")),
            animals: 1,
            welfare: 100,
            food: 100,
            water: 100,
            cleanliness: 100,
            has_shelter: true,
            keeper_id: Some(1),
            next_feed_delivery_minute: None,
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
        assert_eq!(evaluation.cost_cents, habitat_cost(5, 4));

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
                fence_segments(evaluation.x, evaluation.y, width, height),
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
        assert_eq!(state.habitats[0].animals, 6);
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
            assert!((habitat.x + 1..habitat.x + habitat.width - 1).contains(&animal.x));
            assert!((habitat.y + 1..habitat.y + habitat.height - 1).contains(&animal.y));
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
                    x: 3,
                    y: 4,
                    ..test_habitat(3, 4, 5, 3)
                },
            ],
            ..GameState::default()
        };
        state.set_tile(5, ENTRANCE_Y, TileKind::Path);

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
    fn habitat_care_actions_remain_idempotent() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.tick(CARE_DECAY_INTERVAL_MINUTES);
        assert!(state.habitats[0].food < 100);

        let before = state.feed_crates;
        assert!(state.feed_habitat(habitat_id).ok);
        let after_first = state.feed_crates;
        assert_eq!(after_first, before - 1);
        assert!(state.feed_habitat(habitat_id).ok);
        assert_eq!(state.feed_crates, after_first);
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
            "Concession stands must touch a guest path"
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
        state.set_tile(10, 10, TileKind::Path);
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
        state.set_tile(6, ENTRANCE_Y - 2, TileKind::Path);
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
        state.set_tile(10, 10, TileKind::Path);
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

    #[test]
    fn adoption_requires_a_scheduled_keeper() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;

        let unstaffed = state.adopt(habitat_id, "capybara");
        assert!(!unstaffed.ok);
        assert_eq!(
            unstaffed.message,
            "Schedule a keeper before adopting animals"
        );

        assert!(state.hire_keeper().ok);
        assert!(state.schedule_keeper(habitat_id).ok);
        assert!(state.adopt(habitat_id, "capybara").ok);
    }

    #[test]
    fn keeper_assignment_is_idempotent_and_one_keeper_cannot_cover_two_habitats() {
        let mut state = GameState::default();
        for x in 5..=11 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        assert!(state.place_habitat_rect(3, 8, 5, 10).ok);
        assert!(state.place_habitat_rect(7, 8, 9, 10).ok);
        let first_id = state.habitats[0].id;
        let second_id = state.habitats[1].id;

        assert!(state.hire_keeper().ok);
        assert!(state.schedule_keeper(first_id).ok);
        assert!(state.schedule_keeper(first_id).ok);
        assert_eq!(state.keepers.len(), 1);
        assert_eq!(state.keepers[0].assigned_habitat_id, Some(first_id));

        let unavailable = state.schedule_keeper(second_id);
        assert!(!unavailable.ok);
        assert!(unavailable.message.contains("hire another"));

        assert!(state.hire_keeper().ok);
        assert!(state.schedule_keeper(second_id).ok);
        assert_eq!(state.habitats[1].keeper_id, Some(2));
    }

    #[test]
    fn scheduled_delivery_consumes_depot_stock_and_refills_food() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        staff_habitat(&mut state, habitat_id);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.habitats[0].food = 40;
        state.habitats[0].next_feed_delivery_minute = Some(state.absolute_minute() + 1);
        let before = state.feed_crates;

        state.tick(1);

        assert_eq!(state.habitats[0].food, 100);
        assert_eq!(state.feed_crates, before - 1);
        assert_eq!(state.keepers[0].deliveries_completed, 1);
    }

    #[test]
    fn empty_depot_retries_without_creating_free_feed() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        assert!(state.hire_keeper().ok);
        assert!(state.schedule_keeper(habitat_id).ok);
        assert!(state.adopt(habitat_id, "capybara").ok);
        state.habitats[0].food = 40;
        state.habitats[0].next_feed_delivery_minute = Some(state.absolute_minute() + 1);

        state.tick(1);

        assert_eq!(state.feed_crates, 0);
        assert!(state.habitats[0].food < 100);
        assert_eq!(
            state.habitats[0].next_feed_delivery_minute,
            Some(state.absolute_minute() + u64::from(FEED_DELIVERY_RETRY_MINUTES))
        );
    }

    #[test]
    fn bulldozing_habitat_releases_its_keeper() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let habitat_id = state.habitats[0].id;
        assert!(state.hire_keeper().ok);
        assert!(state.schedule_keeper(habitat_id).ok);

        assert!(state.bulldoze(3, 8).ok);
        assert!(state.habitats.is_empty());
        assert_eq!(state.keepers[0].assigned_habitat_id, None);
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
        assert!(state.hire_keeper().ok);
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
        assert!(state.hire_keeper().ok);
        assert!(state.schedule_keeper(habitat_id).ok);
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
            habitat_cost(LEGACY_HABITAT_WIDTH, LEGACY_HABITAT_HEIGHT)
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

        let view = FinanceView::new(state.day, state.finance_today, state.finance_previous);
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
}
