// Shapes of the zoo-core WASM snapshot and command results consumed by the web adapter.

export type Tool =
  | "select"
  | "path"
  | "habitat"
  | "bulldoze"
  | "gate"
  | "zone"
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
  kind: "grass" | "path" | "entrance" | "habitat" | "concession"
  habitat_id: number | null
  concession_id: number | null
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
  orientation: "horizontal" | "vertical"
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
  has_shelter: boolean
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

export type Animal = Point & {
  id: number
  habitat_id: number
  species: SpeciesKey
  slot: number
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

export type TutorialStepKey =
  | "build_habitat"
  | "zone_gate"
  | "assign_keeper"
  | "stock_habitat"
  | "job_generated"
  | "edit_schedule"
  | "job_completed"

/** Rust-owned keeper-training checklist; progress is read from park state, not the DOM. */
export type Tutorial = {
  status: "active" | "skipped" | "completed"
  /** The habitat being taught. */
  habitat_id: number | null
  steps: { key: TutorialStepKey; title: string; done: boolean }[]
  next: TutorialStepKey | null
  lesson: string | null
  /** The park's own blocker for the next step, such as a habitat's waiting reason. */
  problem: string | null
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
}

export type ActionResult = {
  ok: boolean
  message: string
}

export type PlacementEvaluation = {
  ok: boolean
  message: string
  x: number
  y: number
  width: number
  height: number
  orientation: "horizontal" | "vertical"
  cost_cents: number
  occupied_tiles: Point[]
  fence_segments: FenceSegment[]
}
