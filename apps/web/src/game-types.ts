// Shapes of the zoo-core WASM snapshot and command results consumed by the web adapter.

export type Tool =
  | "select"
  | "path"
  | "habitat"
  | "bulldoze"
  | "gate"
  | "reshape"
  | "zone"
  | "terrain"
  | `facility:${string}`
export type Speed = 0 | 1 | 2 | 4
export type SpeciesKey = string
export type ConcessionKind = string
export type FenceSide = "north" | "east" | "south" | "west"

export type Point = {
  x: number
  y: number
}

export type FenceSegment = Point & {
  side: FenceSide
}

export type ViewingSpot = Point & {
  side: FenceSide
  visible_animals: number
  capacity: number
  occupancy: number
  crowded: boolean
}

export type Tile = Point & {
  kind: "grass" | "path" | "entrance" | "habitat" | "concession" | "water"
  habitat_id: number | null
  concession_id: number | null
  /** Who may walk a path tile (#98); null on every other tile. */
  access: PathAccess | null
}

/** Guests keep to public paths; every staff job may also use staff-only paths (#98). */
export type PathAccess = "public" | "staff"

export type ServicePoint = Point & {
  access: PathAccess
  /** Whether staff can walk there from the depot. */
  reachable: boolean
}

export type CareCategory = "feeding" | "cleaning" | "inspection" | "enrichment"

export type CareTask = {
  category: CareCategory
  first_minute_of_day: number
  interval_minutes: number
  priority: number
  work_minutes: number
  customized: boolean
  next_due_in_minutes: number
}

export type CareJob = {
  id: number
  habitat_id: number
  category: CareCategory
  due_day: number
  due_minute_of_day: number
  priority: number
  required_keepers: number
  work_minutes: number
  state: "due" | "in_progress" | "completed" | "missed"
  /** The reserved crew; the first keeper does the care work, any other manages the animals. */
  keeper_ids: number[]
  /** Why a due job has no keeper yet. */
  waiting_reason: string | null
  expires_in_minutes: number
}

export type ServiceGate = FenceSegment & {
  /** The outside tile keepers enter from; it can never be built on. */
  access: Point | null
  reachable: boolean
}

/** Rust's resolution of a tapped tile into a gate-move preview. */
export type GateEvaluation = {
  ok: boolean
  message: string
  segment: FenceSegment | null
  access: Point | null
  /** The work zone the habitat would belong to with its gate on this segment. */
  zone_id: number | null
}

export type ZoneBounds = Point & {
  width: number
  height: number
}

/** A keeper work zone; membership is derived by Rust from habitat service gates. */
export type WorkZone = ZoneBounds & {
  id: number
  habitat_ids: number[]
  keeper_ids: number[]
}

/** Rust's create/resize/move preview for a work zone. */
export type WorkZoneEvaluation = {
  ok: boolean
  message: string
  bounds: ZoneBounds
  habitat_ids: number[]
}

export type Habitat = Point & {
  id: number
  width: number
  height: number
  /** The enclosed tiles, row-major; x, y, width and height are their bounding box. */
  tiles: Point[]
  fence: FenceType
  footprint_area: number
  fence_length: number
  fence_segments: FenceSegment[]
  service_gate: ServiceGate
  species: SpeciesKey | null
  animals: number
  capacity: number
  viewing_capacity: number
  viewing_occupancy: number
  viewing_spots: ViewingSpot[]
  welfare: number
  welfare_target: number
  social_score: number
  space_score: number
  welfare_status: string
  food: number
  water: number
  cleanliness: number
  /** Terrain, elevation, shelters and enrichment, and what the layout offers (#97). */
  environment: HabitatEnvironment
  /** Every weighted part of the welfare target, explained by zoo-core. */
  welfare_parts: WelfarePart[]
  requirements: HabitatRequirements | null
  /** The work zone containing the service gate; null means "No zone". */
  work_zone_id: number | null
  /** Keepers in that zone; only they take this habitat's jobs. */
  zone_keeper_count: number
  care_required_keepers: number | null
  care_schedule: CareTask[]
  care_jobs: CareJob[]
  feeding_status: string
  care_status: string
  appeal: number
}

export type AnimalActivity = "walking" | "resting"

/** One individual animal (#96); every field is authoritative zoo-core state. */
export type Animal = Point & {
  id: number
  habitat_id: number
  species: SpeciesKey
  name: string
  sex: "female" | "male"
  life_stage: "adult"
  arrived_day: number
  activity: AnimalActivity
  /** Where a walking animal is heading. */
  target: Point | null
  energy: number
  /** Presentation-only variation: tile offset and animation phase. */
  cosmetic_seed: number
  animation_phase: number
}

export type Concession = Point & {
  id: number
  kind: ConcessionKind
  build_cost_cents: number
  price_cents: number
  sales_today: number
  total_sales: number
  total_revenue_cents: number
  condition: number
  service_state: "healthy" | "degraded" | "failed"
  maintenance_status: string
  /** The adjacent public tiles guests buy from. */
  customer_tiles: Point[]
  /** Every adjacent path tile a mechanic may work from. */
  service_points: ServicePoint[]
  /** The service point a mechanic from the depot actually uses. */
  active_service_point: Point | null
}

export type Keeper = Point & {
  id: number
  zone_id: number | null
  /** Idle until assigned a work zone. */
  needs_zone: boolean
  /** Care categories the scheduler may give this keeper. */
  categories: CareCategory[]
  /** Daily availability window in minutes of day; an end at or before the start wraps. */
  shift_start_minute: number
  shift_end_minute: number
  on_shift: boolean
  deliveries_completed: number
  /** `awaiting_crew`: inside the gate, waiting for the rest of a dangerous-habitat crew. */
  activity: "at_depot" | "to_gate" | "awaiting_crew" | "working" | "to_depot"
  job_id: number | null
  /** The habitat of the current job. */
  habitat_id: number | null
  /** Rust-owned remaining walk from the keeper's tile to its destination. */
  route: Point[]
  status: string
}

export type Janitor = Point & {
  id: number
  target_litter_id: number | null
  tasks_completed: number
  status: string
}

export type LitterTask = Point & {
  id: number
  age_minutes: number
  assigned_janitor_id: number | null
  status: string
}

export type Mechanic = Point & {
  id: number
  target_maintenance_id: number | null
  repairs_completed: number
  status: string
}

export type MaintenanceTask = Point & {
  id: number
  concession_id: number
  age_minutes: number
  assigned_mechanic_id: number | null
  status: string
}

export type AnimalCareDepot = Point & {
  feed_crates: number
  feed_batch_crates: number
  feed_batch_cost_cents: number
  keeper_hire_cost_cents: number
  keeper_hourly_wage_cents: number
  janitor_hire_cost_cents: number
  janitor_hourly_wage_cents: number
  mechanic_hire_cost_cents: number
  mechanic_hourly_wage_cents: number
  maintenance_repair_cost_cents: number
  keepers: Keeper[]
  janitors: Janitor[]
  mechanics: Mechanic[]
}

export type Guest = Point & {
  id: number
  happiness: number
  energy: number
  hunger: number
  thirst: number
  value_perception: number
  target_habitat: number
  habitats_viewed: number
  state: "arriving" | "walking_to_habitat" | "viewing" | "walking_to_exit"
  thought: string
}

export type FacilityOffer = {
  key: string
  label: string
  service: "food" | "drink" | "entrance" | "animal_care_depot"
  asset_ref: string
  unlock_ref: string
  build_cost_cents: number
  price_cents: number
  footprint: { width: number; height: number }
}

export type SpeciesOffer = {
  asset_ref: string
  key: SpeciesKey
  label: string
  purchase_cost_cents: number
  appeal: number
  minimum_social_group: number
  space_per_animal: number
  /** The lowest habitat fence, in centimetres, this species can be adopted behind. */
  minimum_fence_height_cm: number
  /** What a habitat must offer this species (#97). */
  habitat: HabitatRequirements
}

export type FinanceBreakdown = {
  admissions_income_cents: number
  concession_income_cents: number
  construction_expense_cents: number
  animal_purchase_expense_cents: number
  habitat_care_expense_cents: number
  animal_feed_expense_cents: number
  keeper_hiring_expense_cents: number
  janitor_hiring_expense_cents: number
  mechanic_hiring_expense_cents: number
  maintenance_repair_expense_cents: number
  park_upkeep_expense_cents: number
  keeper_wages_expense_cents: number
  janitor_wages_expense_cents: number
  mechanic_wages_expense_cents: number
}

export type FinanceDay = {
  day: number
  income_cents: number
  expenses_cents: number
  profit_cents: number
  breakdown: FinanceBreakdown
}

/** What a guided objective's hint points at; the HUD maps it to a tool or inspector. */
export type GuideTool =
  | "path"
  | "habitat"
  | "gate"
  | "zone"
  | "depot"
  | "habitat_inspector"
  | "concession"
  | "speed"

/**
 * Rust-owned guide over a guided scenario's objectives (#93); progress is read from park
 * state, not the DOM. `off` means the scenario's objectives are plain goals.
 */
export type Tutorial = {
  status: "off" | "active" | "skipped" | "completed"
  /** The habitat being taught. */
  habitat_id: number | null
  /** The objective the guide points at next. */
  next: string | null
  hint: string | null
  tool: GuideTool | null
  /** The park's own blocker for the next step, or how to recover from a refused command. */
  problem: string | null
}

/** One New park choice from zoo-core's built-in scenario catalog (#91). */
export type ScenarioSummary = {
  id: string
  title: string
  summary: string
  width: number
  height: number
  starting_cash_cents: number
  /** The objectives are a guided introduction with hints. */
  guided: boolean
  objectives: string[]
}

export type Objective = {
  id: string
  title: string
  done: boolean
  focus: Point
  hint: string | null
  tool: GuideTool | null
}

/** The active park's scenario source and its state-backed objectives. */
export type ScenarioState = {
  id: string
  title: string
  seed: number
  opening_focus: Point
  objectives: Objective[]
}

export type Snapshot = {
  ruleset_id: string
  facility_catalog: FacilityOffer[]
  width: number
  height: number
  day: number
  minute_of_day: number
  cash_cents: number
  rating: number
  guest_count: number
  entrance: {
    x: number
    y: number
    arrivals_total: number
  }
  tiles: Tile[]
  habitats: Habitat[]
  concessions: Concession[]
  animal_care_depot: AnimalCareDepot
  animals: Animal[]
  guests: Guest[]
  litter: LitterTask[]
  maintenance: MaintenanceTask[]
  work_zones: WorkZone[]
  operations: {
    cleanliness: number
    litter_backlog: number
    oldest_litter_age_minutes: number
    maintenance_backlog: number
    oldest_maintenance_age_minutes: number
    degraded_concessions: number
    failed_concessions: number
  }
  species_catalog: SpeciesOffer[]
  /** Fence types a habitat can be rebuilt with; the first is what new habitats get. */
  fence_catalog: FenceType[]
  /** Habitat design prices, limits and furnishings (#97). */
  habitat_environment: EnvironmentCatalog
  complaints: {
    hungry: number
    thirsty: number
    tired: number
    poor_value: number
  }
  finance: {
    admission_price_cents: number
    current_day: FinanceDay
    previous_day: FinanceDay | null
    profit_change_cents: number | null
    profit_trend: "up" | "down" | "flat" | "no_previous_day"
  }
  tutorial: Tutorial
  scenario: ScenarioState
  /** Rust-owned bounded construction undo/redo (#92). */
  construction: ConstructionHistory
  /** Rust-owned current park problems, most severe and oldest first (#94). */
  alerts: Alert[]
  summary: DailySummary
}

export type ActionResult = {
  ok: boolean
  message: string
}

/** Rust's plan for a new habitat fence (rectangle or outline) or a reshape of one. */
export type PlacementEvaluation = {
  ok: boolean
  message: string
  /** The habitat being reshaped; null for a new enclosure. */
  habitat_id: number | null
  x: number
  y: number
  width: number
  height: number
  area: number
  perimeter: number
  capacity: number
  cost_cents: number
  occupied_tiles: Point[]
  fence_segments: FenceSegment[]
  added_tiles: Point[]
  removed_tiles: Point[]
  service_gate: FenceSegment | null
  warnings: string[]
  /** The grid corner where a drawn outline fails. */
  blocked_corner: Point | null
}

/** Rust's plan for a path stroke, facility placement or demolition; commits re-plan it. */
export type ConstructionPreview = {
  ok: boolean
  /** False when committing would leave the park unchanged (an existing path, empty grass). */
  changes: boolean
  message: string
  action: "path" | "facility" | "demolish" | "fence"
  cost_cents: number
  tiles: Point[]
  blocked_tiles: Point[]
  affected: { kind: string; id: number | null; effect: string }[]
  undoable: boolean
}

export type ConstructionStep = {
  label: string
  cost_cents: number
  ok: boolean
  message: string
}

export type ConstructionHistory = {
  limit: number
  undo_depth: number
  redo_depth: number
  undo: ConstructionStep | null
  redo: ConstructionStep | null
}

/** What an alert or overlay item concerns (#94). */
export type AlertSubject =
  | { kind: "depot" }
  | { kind: "paths" }
  | { kind: "habitat"; id: number }
  | { kind: "concession"; id: number }
  | { kind: "keeper"; id: number }

export type AlertAction =
  | "buy_feed"
  | "hire_keeper"
  | "hire_janitor"
  | "hire_mechanic"
  | "build_path"
  | "draw_zone"
  | "assign_keeper"
  | "adopt_animals"
  | "inspect_habitat"
  | "inspect_facility"

/**
 * A Rust-owned park alert (#94): one condition on one object, kept while it lasts and
 * removed once resolved. Minutes count from day 1, 00:00.
 */
export type Alert = {
  code: string
  severity: "critical" | "warning" | "notice"
  subject: AlertSubject
  focus: Point
  message: string
  action: AlertAction
  first_minute: number
  last_minute: number
}

export type OverlayKind = "access" | "welfare" | "staff" | "cleanliness" | "facilities" | "viewing"
export type OverlayLevel = "good" | "warn" | "bad"

/** A Rust-graded management overlay; computed on request, never per frame. */
export type OverlayView = {
  kind: OverlayKind
  title: string
  legend: { level: OverlayLevel; label: string }[]
  cells: (Point & { level: OverlayLevel; value: number | null })[]
  items: { level: OverlayLevel; subject: AlertSubject; focus: Point; label: string }[]
  good: number
  warn: number
  bad: number
}

/** Today's headline numbers with the entities and ledger lines that make them up. */
export type DailySummary = {
  day: number
  profit_cents: number
  income_cents: number
  expenses_cents: number
  admissions: number
  admissions_income_cents: number
  admission_price_cents: number
  welfare: number | null
  welfare_habitats: {
    habitat_id: number
    species: SpeciesKey | null
    animals: number
    welfare: number
  }[]
  complaints: number
  complaint_groups: {
    category: "hungry" | "thirsty" | "tired" | "poor_value"
    guest_ids: number[]
  }[]
  unfinished: number
  open_jobs: {
    job_id: number
    habitat_id: number
    category: CareCategory
    state: "due" | "in_progress"
  }[]
  litter: number
  maintenance: number
  headline: string
}

/** A content-defined fence type (#95): look, height and per-segment price and upkeep. */
export type FenceType = {
  key: string
  label: string
  material: "wood" | "steel"
  height_cm: number
  segment_cents: number
  segment_hourly_cents: number
}

/** The ground a habitat tile is made of (#97). */
export type Surface = "grass" | "sand" | "shallow_water" | "deep_water"

export type TerrainCell = Point & { surface: Surface; height: number }

export type HabitatObjectRole = "shelter" | "enrichment"

export type HabitatObject = Point & {
  id: number
  key: string
  label: string
  role: HabitatObjectRole
  capacity: number
  /** An unreachable object provides nothing. */
  reachable: boolean
}

/** What a habitat's layout offers, counted from the service gate by zoo-core. */
export type HabitatAssessment = {
  reachable_tiles: number
  unreachable_tiles: number
  usable_land: number
  cover_tiles: number
  cover_percent: number
  shallow_water: number
  deep_water: number
  shelter_capacity: number
  enrichment_capacity: number
  unreachable_objects: number[]
}

export type HabitatEnvironment = {
  /** Tiles that are not level grass. */
  terrain: TerrainCell[]
  objects: HabitatObject[]
  assessment: HabitatAssessment
  /** Walkable tiles reachable from the gate. */
  reachable: Point[]
}

export type WelfarePart = {
  key: string
  label: string
  weight: number
  score: number
  met: boolean
  detail: string
}

export type HabitatRequirements = {
  ground_cover: Surface
  ground_cover_percent: number
  shallow_water_tiles: number
  deep_water_tiles: number
  shelter: boolean
  enrichment: boolean
}

export type HabitatObjectType = {
  key: string
  label: string
  role: HabitatObjectRole
  build_cents: number
  capacity: number
}

export type EnvironmentCatalog = {
  surfaces: { key: Surface; label: string; tile_cents: number; water: boolean }[]
  elevation_step_cents: number
  max_height: number
  max_walk_step: number
  max_slope_step: number
  max_objects: number
  objects: HabitatObjectType[]
}

/** Rust's plan for one habitat environment edit; the command plans again. */
export type EnvironmentPreview = {
  ok: boolean
  message: string
  habitat_id: number
  cost_cents: number
  changed_tiles: Point[]
  blocking_tiles: Point[]
  assessment: HabitatAssessment | null
  welfare_target: number | null
}
