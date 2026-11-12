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
  | "land"
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

/** A care job's work: routine keeper care, or a vet's treatment of one animal (#109). */
export type CareJobCategory = CareCategory | "treatment" | "talk"

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
  category: CareJobCategory
  due_day: number
  due_minute_of_day: number
  priority: number
  required_keepers: number
  work_minutes: number
  state: "due" | "in_progress" | "completed" | "missed"
  /** The reserved crew; the first keeper does the care work, any other manages the animals. */
  keeper_ids: number[]
  /** Why a due job has no keeper yet, or why its dispatched crew cannot move. */
  waiting_reason: string | null
  expires_in_minutes: number
  /** Rust-owned lifecycle stage (#104); `queued` waits for busy keepers, `blocked` for a fix. */
  phase: CarePhase
  /** The job's hold on supplies: reserved at the depot, carried, or bought on completion. */
  supply:
    | "none"
    | "feed_reserved"
    | "feed_carried"
    | "cleaning"
    | "cleaning_prepaid"
    | "medicine_reserved"
    | "medicine_carried"
  /** Minutes since the job fell due while it is open. */
  waited_minutes: number
  /** Tiles left on the crew's current walk (or depot to gate while due); null without a route. */
  route_tiles: number | null
  /** Where inside the habitat the care is performed. */
  care_spot: Point | null
  /** The patient of a treatment (#109). */
  animal_id: number | null
}

export type CarePhase =
  | "queued"
  | "blocked"
  | "collecting_stock"
  | "traveling"
  | "reserved"
  | "servicing"
  | "completed"
  | "missed"

/** The park's care backlog and where its feed is (#104). */
export type CareBacklog = {
  open: number
  queued: number
  blocked: number
  in_progress: number
  oldest_wait_minutes: number
  completed_recent: number
  missed_recent: number
  feed_on_shelf: number
  feed_reserved: number
  feed_carried: number
  supply_shortages: number
  /** Medicine kits on the shelf, earmarked and in vets' hands; open treatments (#109). */
  medicine_on_shelf: number
  medicine_reserved: number
  medicine_carried: number
  treatments_open: number
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
  /** Resident species keys, in arrival order (#108). */
  species: SpeciesKey[]
  /** Each resident species with its own animals, welfare and requirements (#108). */
  residents: Resident[]
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
  /** Set aside for quarantine: it takes only unwell animals (#109). */
  quarantine: boolean
  /** Pool water and climate control: demand, supply and why it is short (#113). */
  utilities: HabitatUtilities
}

export type UtilityKind = "water" | "climate"
export type UtilityPriority = "high" | "normal" | "low"
export type ShortageReason = "none_built" | "unconnected" | "failed" | "overloaded"

/** One utility's allocation to a habitat, computed by zoo-core (#113). */
export type UtilitySupply = {
  kind: UtilityKind
  label: string
  demand: number
  supplied: number
  /** `short` is the warning hour; `degraded` lowers welfare until it is fixed. */
  status: "not_needed" | "supplied" | "short" | "degraded"
  reason: ShortageReason | null
  detail: string
  fix: string | null
  shortage_minutes: number
  /** Place in this utility's service order, from 1; 0 without demand. */
  rank: number
  draws: { concession_id: number; units: number }[]
  /** Plants of this kind the habitat reaches, nearest first. */
  reachable: number[]
}

export type HabitatUtilities = {
  priority: UtilityPriority
  water: UtilitySupply
  climate: UtilitySupply
}

/** A utility plant's capacity, allocation and failure state (#113). */
export type UtilityFacility = {
  kind: UtilityKind
  capacity: number
  coverage_tiles: number
  allocated: number
  working: boolean
  overloaded: boolean
  short_habitats: number[]
  consumers: { habitat_id: number; units: number; rank: number; priority: UtilityPriority }[]
  hourly_operating_cents: number
  replacement_cents: number
  can_replace: boolean
  service_minutes: number
  status: string
}

/** One resident species of a habitat, summarized by zoo-core from its own animals (#108). */
export type Resident = {
  species: SpeciesKey
  label: string
  animals: number
  minimum_social_group: number
  welfare: number
  welfare_target: number
  social_score: number
  requirements: HabitatRequirements
  welfare_parts: WelfarePart[]
}

/** A pairing verdict between two species, authored in the content (#108). */
export type PairingVerdict = {
  species: [SpeciesKey, SpeciesKey]
  compatible: boolean
  reason: string
}

/** A shared habitat place before and after an arrival or departure. */
export type ResourceShare = {
  provided: number
  needed_before: number
  needed_after: number
}

export type ResidentEffect = {
  species: SpeciesKey
  label: string
  animals_before: number
  animals_after: number
  minimum_social_group: number
  welfare: number | null
  welfare_target_before: number | null
  welfare_target_after: number | null
}

export type HabitatEffect = {
  habitat_id: number
  animals_before: number
  animals_after: number
  capacity: number
  space: ResourceShare
  shelter: ResourceShare
  enrichment: ResourceShare
  needs_per_hour_before: number
  needs_per_hour_after: number
  upkeep_cents_per_hour_before: number
  upkeep_cents_per_hour_after: number
  residents: ResidentEffect[]
}

/** zoo-core's adoption or transfer preview (#108); it changes nothing. */
export type ArrivalPreview = {
  ok: boolean
  message: string
  blockers: string[]
  warnings: string[]
  species: SpeciesKey
  label: string
  count: number
  cost_cents: number
  pairings: PairingVerdict[]
  destination: HabitatEffect | null
  source: HabitatEffect | null
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
  /** Its species' welfare in its habitat (#108). */
  welfare: number
  /** Presentation-only variation: tile offset and animation phase. */
  cosmetic_seed: number
  animation_phase: number
  /** Risk, illness, treatment and quarantine (#109). */
  health: AnimalHealth
}

export type HealthCondition = "upset_tummy" | "sniffles"

/** zoo-core's view of one animal's health (#109); illness is gentle and always recoverable. */
export type AnimalHealth = {
  status: "healthy" | "at_risk" | "unwell" | "recovering"
  condition: HealthCondition | null
  /** What is wrong, why, and what happens next. */
  summary: string
  /** The highest risk as a share of onset. */
  risk_percent: number
  recovery_percent: number | null
  /** Why recovery is paused, if it is. */
  recovery_paused: string | null
  treatment: {
    job_id: number
    phase: CarePhase
    vet_id: number | null
    waiting_reason: string | null
    /** The fix for a waiting treatment. */
    action: AlertAction | null
  } | null
  in_quarantine: boolean
  quarantine_home: number | null
  /** Why a recovered patient cannot go home yet. */
  release_blocked: string | null
}

/** zoo-core's preview of moving an ill animal into quarantine (#109). */
export type QuarantinePreview = {
  ok: boolean
  message: string
  blockers: string[]
  habitat_id: number | null
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
  /** Toilets, benches and bins (#101); `null` for stands. */
  comfort: ComfortFacility | null
  /** Food and drink stands (#102); `null` for comfort facilities. */
  stand: StandOperations | null
  /** Water pumps and climate plants (#113); `null` for every other facility. */
  utility: UtilityFacility | null
  /** An education sign: what it teaches, or why it teaches nothing (#116); absent otherwise. */
  sign?: SignState
}

/** Why a sale was lost, counted by zoo-core per stand (#102). */
export type LostDemand = {
  queue_full: number
  gave_up: number
  unaffordable: number
  /** The price was above what the guest thought fair: a price complaint (#106). */
  too_expensive: number
  closed: number
  unstaffed: number
  out_of_stock: number
  broken_down: number
}

export type StandLedger = {
  served: number
  revenue_cents: number
  average_wait_minutes: number | null
  lost_sales: number
  lost: LostDemand
  stock_cost_cents: number
  wage_cents: number
  upkeep_cents: number
  repair_cents: number
  operating_cents: number
  contribution_cents: number
}

/** Rust-owned vendors, queue, stock and operating figures of a food or drink stand (#102). */
export type StandOperations = {
  status:
    | "open"
    | "restocking"
    | "closed"
    | "broken_down"
    | "unstaffed"
    | "out_of_stock"
    | "unreachable"
  status_label: string
  open: boolean
  /** The price now, and the authored reference price guests judge it against (#106). */
  price_cents: number
  reference_price_cents: number
  vendors: number
  max_vendors: number
  vendor_hourly_wage_cents: number
  serving: number
  queue: number
  queue_capacity: number
  heading: number
  service_minutes: number
  capacity_per_hour: number
  stock: number
  stock_capacity: number
  restock_at: number
  auto_restock: boolean
  delivery_in_minutes: number | null
  refill_cost_cents: number
  stock_unit_cost_cents: number
  today: StandLedger
  total: StandLedger
}

/** Rust-owned state of a toilet, bench or litter bin (#101). */
export type ComfortFacility = {
  service: "toilet" | "bench" | "bin"
  capacity: number
  in_use: number
  waiting: number
  heading: number
  open: boolean
  cleanliness: number | null
  fill: number | null
  total_uses: number
  status: string
  janitor_status: string | null
}

export type ComfortOperations = {
  facility_service_backlog: number
  oldest_facility_service_age_minutes: number
  toilet_demand_unmet: number
  rest_demand_unmet: number
}

export type FacilityServiceTask = Point & {
  id: number
  concession_id: number
  work: "clean_toilet" | "empty_bin"
  age_minutes: number
  assigned_janitor_id: number | null
  status: string
}

export type Keeper = Point & {
  id: number
  /** Keepers work their zone's routine care; vets treat unwell animals anywhere (#109). */
  role: "keeper" | "vet"
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
  /**
   * `collecting_stock`: walking to the depot for a feed crate; `awaiting_crew`: inside the gate,
   * waiting for the rest of a dangerous-habitat crew; `to_care_spot`: walking inside the habitat
   * to the feeder or task location.
   */
  activity:
    | "at_depot"
    | "collecting_stock"
    | "to_gate"
    | "awaiting_crew"
    | "to_care_spot"
    | "working"
    | "to_depot"
  /** Whether the keeper carries a feed crate. */
  carrying_feed: boolean
  /** Whether the vet carries a medicine kit (#109). */
  carrying_medicine: boolean
  job_id: number | null
  /** The habitat of the current job. */
  habitat_id: number | null
  /** Rust-owned remaining walk from the keeper's tile to its destination. */
  route: Point[]
  status: string
}

export type Janitor = Point & {
  id: number
  /** The cleanup job being walked to: litter on a path or a toilet/bin service task. */
  target: { litter: number } | { facility: number } | null
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
  /** Medicine stock and vet costs (#109). */
  medicine_kits: number
  medicine_batch_kits: number
  medicine_batch_cost_cents: number
  vet_hire_cost_cents: number
  vet_hourly_wage_cents: number
  care_backlog: CareBacklog
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
  state:
    | "arriving"
    | "walking_to_habitat"
    | "viewing"
    | "walking_to_exit"
    | "walking_to_facility"
    | "waiting_for_facility"
    | "using_facility"
  thought: string
  /** Toilet need, current facility and the need the park last failed to meet (#101). */
  comfort: {
    bladder: number
    facility_id: number | null
    unmet_need: "toilet" | "rest" | "food" | "drink" | null
  }
  /** Spending money left for stands (#102). */
  cash_cents: number
  /** The habitats this guest has watched, in order (#103). */
  visited_habitats: number[]
  /** The guest's group, its profile and current plan (#103). */
  group: GuestGroup | null
  /** Minutes spent in crowds so far, and whether the guest stands in one now (#103). */
  crowd_minutes: number
  crowded: boolean
  /** Why the guest is walking out, once decided (#103). */
  exit: ExitCause | null
  /** Learning points this visit and what they were learned about (#116). */
  learning: number
  lessons: Lesson[]
}

export type VisitorProfile = "family" | "enthusiast" | "stroller"

export type GuestGroup = {
  id: number
  profile: VisitorProfile
  profile_label: string
  favorite_species: SpeciesKey | null
  members: number[]
  /** People admitted with the group; 0 for a member who carried on alone. */
  admitted: number
  visit_goal: number
  walking_tolerance: number
  patience: number
  plan: string
  waiting_for_group: boolean
  waiting_minutes: number
}

export type ExitReason =
  | "satisfied"
  | "needs_unmet"
  | "too_crowded"
  | "poor_value"
  | "no_reachable_attraction"

export type ExitCause = { reason: ExitReason; detail: string }

export type ExitCounts = Record<ExitReason, number>

/** Groups, arrivals, exit reasons and crowded tiles (#103). */
export type VisitsState = {
  guests_in_park: number
  groups_in_park: number
  groups_arrived: number
  guests_arrived: number
  today: ExitCounts
  total: ExitCounts
  recent: {
    guest_id: number
    group_id: number
    minute: number
    reason: ExitReason
    detail: string
  }[]
  crowded_tiles: { x: number; y: number; guests: number }[]
}

export type FacilityOffer = {
  key: string
  label: string
  service:
    | "food"
    | "drink"
    | "toilet"
    | "bench"
    | "bin"
    | "water_pump"
    | "climate_plant"
    | "sign"
    | "entrance"
    | "animal_care_depot"
  asset_ref: string
  unlock_ref: string
  build_cost_cents: number
  price_cents: number
  /** Toilet cubicles, bench seats or the litter a bin holds (#101); 0 for stands. */
  capacity: number
  footprint: { width: number; height: number }
  /** A food or drink stand's service model (#102); absent for every other facility. */
  stand?: {
    service_minutes: number
    queue_capacity: number
    max_vendors: number
    stock_capacity: number
    stock_unit_cost_cents: number
  }
  /** What a utility plant supplies (#113); absent for every other facility. */
  utility?: { kind: UtilityKind; capacity: number; coverage_tiles: number }
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
  /** Guests' donations and scenario grants (#116). */
  donation_income_cents: number
  grant_income_cents: number
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
  /** Parcels bought (#99). */
  land_expense_cents: number
  /** Stand vendors' wages and stock deliveries (#102). */
  vendor_wages_expense_cents: number
  stand_stock_expense_cents: number
  /** Marketing campaigns (#106). */
  marketing_expense_cents: number
  /** Veterinary care (#109). */
  vet_hiring_expense_cents: number
  vet_wages_expense_cents: number
  medicine_expense_cents: number
  /** Utility plants (#113). */
  utility_operation_expense_cents: number
  utility_replacement_expense_cents: number
  /** Loan interest (an expense), proceeds and principal repayments (financing) (#115). */
  loan_interest_expense_cents: number
  loan_proceeds_cents: number
  loan_repayment_cents: number
}

export type FinanceDay = {
  day: number
  /** Cash when the day began and when it closed (or now, for today) (#115). */
  opening_cash_cents: number
  closing_cash_cents: number
  /** What the park owed at the close (or now). */
  loan_balance_cents: number
} & FinanceFlows

/** One ledger's flows by kind (#115): operating, investment and financing. */
export type FinanceFlows = {
  /** Operating income: admissions and concessions. */
  income_cents: number
  /** Operating, investment and interest expenses; never loan principal. */
  expenses_cents: number
  profit_cents: number
  operating_expenses_cents: number
  operating_result_cents: number
  investment_cents: number
  interest_cents: number
  loan_proceeds_cents: number
  loan_repayments_cents: number
  /** Cash borrowing provided: proceeds less repayments. */
  net_borrowing_cents: number
  net_cash_flow_cents: number
  verdict: FinanceVerdict
  breakdown: FinanceBreakdown
}

export type FinanceVerdict =
  | "operations_profitable"
  | "borrowing_covers_losses"
  | "operations_losing"
  | "no_activity"

export type FinancePeriod = {
  period_days: number
  /** Completed days the period actually covers. */
  days: number
  first_day: number | null
  last_day: number | null
  opening_cash_cents: number | null
  closing_cash_cents: number | null
  flows: FinanceFlows
}

export type FinanceLine = { line: keyof FinanceBreakdown; cents: number }

export type RecoveryStep =
  | { kind: "raise_admission"; admission_cents: number; fair_cents: number }
  | {
      kind: "cut_vendors"
      stand_id: number
      vendors: number
      wages_per_day_cents: number
      margin_today_cents: number
    }
  | { kind: "postpone_expansion"; invested_cents: number }
  | { kind: "rehome_animals"; animal_upkeep_per_day_cents: number }
  | { kind: "borrow"; headroom_cents: number }

/** Rust-owned finances report (#39, #115). */
export type FinanceState = {
  admission_price_cents: number
  current_day: FinanceDay
  previous_day: FinanceDay | null
  profit_change_cents: number | null
  profit_trend: "up" | "down" | "flat" | "no_previous_day"
  solvency: {
    status: "healthy" | "low" | "deficit"
    low_cash_line_cents: number
    deficit_cents: number
    deficit_days: number
    deficit_streak_days: number
    spending_blocked: boolean
    recovery: RecoveryStep[]
  }
  loan: {
    available: boolean
    limit_cents: number
    daily_interest_basis_points: number
    balance_cents: number
    headroom_cents: number
    interest_tonight_cents: number
    interest_per_day_cents: number
    borrowed_total_cents: number
    repaid_total_cents: number
    interest_total_cents: number
  }
  budget: {
    commitments: FinanceLine[]
    committed_per_day_cents: number
    basis_days: number
    expected_income_per_day_cents: number
    expected_supplies_per_day_cents: number
    projected_net_per_day_cents: number
    /** Whole days cash lasts at the projected rate; null while it is not running down. */
    runway_days: number | null
    drivers: FinanceLine[]
    assumptions: string[]
  }
  /** The last 7 and 30 completed days. */
  periods: FinancePeriod[]
  /** Kept completed days, newest first (at most 30). */
  history: FinanceDay[]
}

/** zoo-core's budget plan for candidate spending and borrowing, or why it cannot be made. */
export type BudgetEvaluation =
  | {
      ok: true
      plan: {
        spend_cents: number
        borrow_cents: number
        cash_after_cents: number
        affordable: boolean
        within_limit: boolean
        loan_balance_after_cents: number
        interest_per_day_after_cents: number
        projected_net_per_day_cents: number
        runway_days: number | null
      }
    }
  | { ok: false; message: string }

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
  /** Open janitor work at toilets and bins (#101). */
  facility_service: FacilityServiceTask[]
  work_zones: WorkZone[]
  operations: {
    cleanliness: number
    litter_backlog: number
    oldest_litter_age_minutes: number
    maintenance_backlog: number
    oldest_maintenance_age_minutes: number
    degraded_concessions: number
    failed_concessions: number
    /** Janitor backlog at toilets and bins and unmet guest demand (#101). */
    comfort: ComfortOperations
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
    /** Guests who found no usable toilet or bench on their last try (#101). */
    needs_toilet: number
    needs_rest: number
  }
  finance: FinanceState
  tutorial: Tutorial
  scenario: ScenarioState
  /** Rust-owned bounded construction undo/redo (#92). */
  construction: ConstructionHistory
  /** Rust-owned current park problems, most severe and oldest first (#94). */
  alerts: Alert[]
  summary: DailySummary
  /** Purchasable parcels, ownership and restrictions (#99). */
  land: LandState
  /** Groups, arrivals, exit reasons and crowded tiles (#103). */
  visits: VisitsState
  /** Prices, the gate, observed demand and marketing (#106). */
  pricing: PricingState
  /** Signs, talks, learning, donations, conservation milestones and grants (#116). */
  education: EducationState
}

export type ParcelStatus = "owned" | "available" | "unaffordable" | "locked"

/** One scenario-authored parcel as zoo-core presents it (#99). */
export type Parcel = {
  id: string
  title: string
  summary: string
  areas: { x: number; y: number; width: number; height: number }[]
  price_cents: number
  owned: boolean
  status: ParcelStatus
  /** Why the parcel cannot be bought right now. */
  restriction: string | null
  /** The unlock condition, met or not. */
  unlock: string | null
  tiles: number
  buildable_tiles: number
  water_tiles: number
  /** An existing public path or the entrance touches the parcel. */
  path_frontage: boolean
  price_per_buildable_tile_cents: number | null
}

export type LandState = {
  parcels: Parcel[]
  owned_tiles: number
  total_tiles: number
}

/** Rust's quote for buying a selection of parcels together; `buy_land` needs its token. */
export type LandQuote = {
  ok: boolean
  message: string
  lines: {
    id: string
    title: string | null
    price_cents: number
    status: "available" | "owned" | "locked" | "unknown" | "duplicate"
    reason: string | null
  }[]
  total_cents: number
  cash_after_cents: number
  quote: string
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
  /** Unbought parcels the enclosure needs first (#99). */
  land_parcels: string[]
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
  /** Unbought parcels the footprint needs first (#99). */
  land_parcels: string[]
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
  | { kind: "animal"; id: number }
  | { kind: "finances" }

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
  | "build_toilet"
  | "build_bench"
  | "hire_vet"
  | "buy_medicine"
  | "inspect_animal"
  | "build_water_pump"
  | "build_climate_plant"
  | "open_finances"

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

export type OverlayKind =
  | "access"
  | "welfare"
  | "staff"
  | "cleanliness"
  | "facilities"
  | "viewing"
  | "utilities"
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
    species: SpeciesKey[]
    animals: number
    welfare: number
  }[]
  complaints: number
  complaint_groups: {
    category: "hungry" | "thirsty" | "tired" | "poor_value" | "needs_toilet" | "needs_rest"
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
  /** Open janitor work at toilets and bins (#101). */
  facility_service: number
  /** People who walked out today, and why (#103). */
  exits: number
  exit_reasons: ExitCounts
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

export type GateOutcome = "admitted" | "closed" | "too_expensive" | "over_budget"

/** Arrivals at the gate over a period (#106): potential arrivals, admissions and rejections. */
export type GateCounts = {
  groups_at_gate: number
  people_at_gate: number
  groups_admitted: number
  people_admitted: number
  turned_away_closed: number
  turned_away_too_expensive: number
  turned_away_over_budget: number
  admission_cents: number
}

export type GateNote = {
  arrival: number
  minute: number
  profile: VisitorProfile
  size: number
  price_cents: number
  value_cents: number
  willing_cents: number
  outcome: GateOutcome
}

/** zoo-core's forecast for one admission price, with its assumptions (#106). */
export type AdmissionForecast = {
  price_cents: number
  sample_groups: number
  sample_people: number
  accepting_groups: number
  accepting_people: number
  admission_cents: number
  assumptions: string
}

export type AdmissionEvaluation =
  | { ok: true; forecast: AdmissionForecast }
  | { ok: false; message: string }

export type StandPrice = {
  id: number
  kind: string
  label: string
  price_cents: number
  reference_price_cents: number
  served_today: number
  revenue_today_cents: number
  too_expensive_today: number
  unaffordable_today: number
  lost_today: number
  contribution_today_cents: number
  contribution_total_cents: number
}

/** Prices, the gate, observed demand and marketing (#106). */
export type PricingState = {
  open: boolean
  admission_price_cents: number
  max_admission_cents: number
  max_stand_price_cents: number
  experience_value_cents: number
  valued_habitats: number
  today: GateCounts
  total: GateCounts
  turned_away_today: number
  price_complaints_today: number
  poor_value_exits_today: number
  average_value: number | null
  income_today_cents: number
  profit_today_cents: number
  forecast: AdmissionForecast
  recent: GateNote[]
  stands: StandPrice[]
  campaign: { ends_in_minutes: number } | null
  campaign_cost_cents: number
  campaign_minutes: number
  campaign_arrival_boost_percent: number
}

/** What one guest learned about one species (#116). */
export type Lesson = {
  species: string
  habitat_id: number
  source: "sign" | "talk"
  points: number
}

/** An authored species fact and its source (#116). */
export type SpeciesFact = {
  species: string
  fact: string
  source_title: string
  source_url: string
}

/** What an education sign teaches, or why it teaches nothing (#116). */
export type SignState = Point & {
  id: number
  habitat_id: number | null
  species: string[]
  status: "teaching" | "decorative" | "unreachable"
  detail: string
  facts: SpeciesFact[]
}

export type TalkOutcome = {
  habitat_id: number
  minute: number
  given: boolean
  attendees: number
  detail: string
}

/** A habitat's daily keeper talk and where it stands (#116). */
export type TalkState = {
  habitat_id: number
  species: string
  start_minute_of_day: number
  duration_minutes: number
  places: number
  next_due_minute: number
  status: "scheduled" | "waiting_for_keeper" | "keeper_on_the_way" | "in_progress" | "blocked"
  detail: string
  attendees: number
  last: TalkOutcome | null
}

export type EducationCounts = {
  guests_educated: number
  sign_lessons: number
  talk_lessons: number
  talks_given: number
  talks_canceled: number
  attendees: number
  donors: number
  donations_cents: number
  grants_cents: number
}

export type AchievementKind = "thriving_exhibit" | "vet_recovery" | "supported_rehoming"

export type EarnedAchievement = {
  kind: AchievementKind
  minute: number
  habitat_id: number
  animal_ids: number[]
  detail: string
}

/** A conservation milestone: earned once, from real animals and events (#116). */
export type AchievementState = {
  kind: AchievementKind
  title: string
  description: string
  earned: EarnedAchievement | null
  progress: string
}

export type Rehoming = {
  animal_id: number
  minute: number
  eligible: boolean
  detail: string
}

/** A scenario grant and its explicit condition (#116). */
export type GrantState = {
  id: string
  title: string
  condition: string
  amount_cents: number
  awarded_minute: number | null
  progress: string
}

/** The Rust-owned conservation and education report (#116). */
export type EducationState = {
  signs: SignState[]
  talks: TalkState[]
  learning: {
    learners_now: number
    average_learning: number
    today: EducationCounts
    total: EducationCounts
  }
  donations: { habitat_id: number; label: string; donors: number; cents: number }[]
  recent_talks: TalkOutcome[]
  achievements: AchievementState[]
  recent_rehomings: Rehoming[]
  grants: GrantState[]
  reasons: string[]
}
