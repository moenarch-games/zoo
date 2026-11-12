//! Habitat-owned recurring care schedules and the stable jobs they generate (#133).
//!
//! A habitat copies its species' care defaults when first stocked and owns that schedule
//! afterwards. Each scheduled occurrence becomes exactly one job keyed by habitat, category
//! and due minute. A job stays eligible for one in-game hour after it is due, then expires as
//! missed unless a keeper was dispatched to it within that hour.
//!
//! The keeper scheduler (#134) matches due jobs to keepers every minute. A keeper is eligible
//! for a job only when it works in the zone holding the habitat's service gate, accepts the
//! job's category, is on shift, and is free (at or returning to the depot). There is no
//! cross-zone borrowing or park-wide fallback. Jobs are served highest effective priority
//! first, then oldest due, then lowest id; each job takes the eligible keeper with the
//! shortest walk to the gate (lowest id on ties). Dispatch reserves the keeper and the job's
//! supplies at once, so neither a keeper nor a feed crate can be claimed twice.
//!
//! Dangerous species need a crew (#136): every in-habitat job of such a habitat stays one job
//! with a worker requirement of two. Dispatch reserves the two nearest eligible keepers
//! together or none at all; the first to arrive waits inside the gate for the other, and
//! once both are there they work together: the first reserved keeper does the care work while
//! the other manages the animals. A crew that cannot be assembled within the grace hour
//! leaves the job missed.
//!
//! Keepers walk (#95, #104): care is physical. A feeding job reserves one feed crate at the
//! nearest reachable depot with unreserved stock at dispatch (the park has one depot, so that
//! is the depot or none); the worker walks there to collect it unless already standing there,
//! carries it along paths to the habitat's service gate, enters, and walks through the
//! habitat's reachable ground to the job's care spot (the feeding station, the far corner to
//! clean, the shelter to inspect, the enrichment item to service). The crew assembles inside
//! the gate, walks to the spot together, works there for the task's `work_minutes`, and only
//! then is the care applied and the crate consumed. Cleaning supplies are bought exactly once,
//! when the cleaning is done. Then the keeper takes the next job or walks back out.
//!
//! Stock and jobs reconcile when the world changes under a dispatched job: a crew member
//! moved to another zone (or the habitat's gate moved out of the crew's zone) interrupts the
//! job, and so does a crew that has had no route since the job's grace hour lapsed. An
//! interrupted job releases its reservation, returns to due while its grace hour lasts (else
//! it is missed), and a keeper carrying its crate walks the crate back to the depot. Feed is
//! therefore conserved: every bought crate is on the depot shelf, carried by exactly one
//! keeper, or consumed by exactly one completed feeding.
use super::{
    ActionResult, ExpenseCategory, FEED_DELIVERY_THRESHOLD, GameState, Keeper, Mover, Position,
    TileKind,
    content::{self, CareCategory, CareProfile},
};
use serde::{Deserialize, Serialize};

/// What a keeper is doing; positions live on the keeper itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeeperActivity {
    AtDepot,
    /// Walking to the depot to collect the job's reserved feed crate (#104).
    CollectingStock {
        job_id: u32,
    },
    ToGate {
        job_id: u32,
    },
    /// Inside the gate, waiting for the rest of the job's crew to arrive.
    AwaitingCrew {
        job_id: u32,
    },
    /// Inside the habitat, walking from the gate to the job's care spot (#104).
    ToCareSpot {
        job_id: u32,
    },
    Working {
        job_id: u32,
        remaining_minutes: u32,
    },
    ToDepot,
}

impl KeeperActivity {
    pub fn key(self) -> &'static str {
        match self {
            Self::AtDepot => "at_depot",
            Self::CollectingStock { .. } => "collecting_stock",
            Self::ToGate { .. } => "to_gate",
            Self::AwaitingCrew { .. } => "awaiting_crew",
            Self::ToCareSpot { .. } => "to_care_spot",
            Self::Working { .. } => "working",
            Self::ToDepot => "to_depot",
        }
    }

    /// Free keepers can take a new job: at the depot or on the way back to it.
    pub fn is_free(self) -> bool {
        matches!(self, Self::AtDepot | Self::ToDepot)
    }

    /// Whether the keeper is walking, so it moves along a route each staff movement step.
    pub fn is_walking(self) -> bool {
        matches!(
            self,
            Self::CollectingStock { .. }
                | Self::ToGate { .. }
                | Self::ToCareSpot { .. }
                | Self::ToDepot
        )
    }

    pub fn job_id(self) -> Option<u32> {
        match self {
            Self::CollectingStock { job_id }
            | Self::ToGate { job_id }
            | Self::AwaitingCrew { job_id }
            | Self::ToCareSpot { job_id }
            | Self::Working { job_id, .. } => Some(job_id),
            Self::AtDepot | Self::ToDepot => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Supply {
    None,
    FeedCrate,
    CleaningFee,
}

/// What a job holds of the park's supplies (#104). Reserving never moves stock; collecting a
/// crate moves it from the depot shelf to the worker; completing the care consumes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobSupply {
    /// Nothing to bring: the animals were not hungry, the habitat was clean, or the task
    /// needs no supplies.
    #[default]
    None,
    /// One crate on the depot shelf is earmarked for this job; the worker has not collected it.
    FeedReserved,
    /// The worker carries the job's crate (once completed: the crate was fed).
    FeedCarried,
    /// Cleaning supplies are bought when the cleaning is done.
    Cleaning,
    /// Cleaning supplies already paid at dispatch by a schema 14 save, never charged again.
    CleaningPrepaid,
}

/// Where a job stands for the player (#104): due jobs are `queued` while keepers are merely
/// busy or off shift and `blocked` while the park itself prevents them; dispatched jobs are
/// `collecting_stock`, `traveling` (to the gate or care spot), `reserved` (the worker waits
/// for its crew), `servicing`, or `blocked` when a crew member has no route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CarePhase {
    Queued,
    Blocked,
    CollectingStock,
    Traveling,
    Reserved,
    Servicing,
    Completed,
    Missed,
}

/// Why due care is waiting, and the fix the player would make.
pub(crate) struct CareWait {
    pub message: String,
    /// Whether the park prevents the job (rather than keepers merely being busy).
    pub blocked: bool,
    pub action: crate::alerts::AlertAction,
    /// Whether the depot (or the cash for supplies) is what the job waits for.
    pub supply_shortage: bool,
}

/// Which jobs and hours a keeper works: availability windows and accepted categories, never
/// a fixed route through habitats.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeeperSchedule {
    /// Accepted categories, deduplicated in catalog order; never empty.
    pub categories: Vec<CareCategory>,
    pub shift_start_minute: u32,
    /// Exclusive; at or before the start the shift wraps past midnight.
    pub shift_end_minute: u32,
}

impl Default for KeeperSchedule {
    fn default() -> Self {
        Self {
            categories: CareCategory::ALL.to_vec(),
            shift_start_minute: 0,
            shift_end_minute: MINUTES_PER_DAY as u32,
        }
    }
}

impl KeeperSchedule {
    pub fn on_shift(&self, minute_of_day: u32) -> bool {
        let (start, end) = (self.shift_start_minute, self.shift_end_minute);
        if start < end {
            (start..end).contains(&minute_of_day)
        } else {
            minute_of_day >= start || minute_of_day < end
        }
    }

    pub fn accepts(&self, category: CareCategory) -> bool {
        self.categories.contains(&category)
    }
}

pub(crate) const CARE_GRACE_MINUTES: u64 = 60;
/// Resolved jobs stay visible for one in-game day, which bounds the job list.
const RESOLVED_JOB_RETENTION_MINUTES: u64 = 24 * 60;
const MINUTES_PER_DAY: u64 = 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CareTask {
    pub category: CareCategory,
    pub first_minute_of_day: u32,
    pub interval_minutes: u32,
    pub priority: u32,
    pub work_minutes: u32,
    /// Whether the player changed this task away from its species default.
    pub customized: bool,
    /// The absolute minute of the next occurrence; saved, never shown.
    pub next_due_minute: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CareSchedule {
    pub required_keepers: u32,
    pub tasks: Vec<CareTask>,
}

impl CareSchedule {
    pub fn instantiate(profile: &CareProfile, now: u64) -> Self {
        Self {
            required_keepers: profile.required_keepers,
            tasks: profile
                .schedule
                .iter()
                .map(|task| CareTask {
                    category: task.category,
                    first_minute_of_day: task.first_minute_of_day,
                    interval_minutes: task.interval_minutes,
                    priority: task.priority,
                    work_minutes: task.work_minutes,
                    customized: false,
                    next_due_minute: next_occurrence(
                        task.first_minute_of_day,
                        task.interval_minutes,
                        now,
                    ),
                })
                .collect(),
        }
    }
}

fn clock(minute_of_day: u32) -> String {
    format!("{:02}:{:02}", minute_of_day / 60 % 24, minute_of_day % 60)
}

/// The first occurrence at or after `now` of a cadence anchored at `first_minute_of_day`.
fn next_occurrence(first_minute_of_day: u32, interval_minutes: u32, now: u64) -> u64 {
    let interval = u64::from(interval_minutes);
    let day_start = now - now % MINUTES_PER_DAY;
    let offset = u64::from(first_minute_of_day) % interval;
    let since = (now - day_start).saturating_sub(offset);
    day_start + offset + since.div_ceil(interval) * interval
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CareJobState {
    Due,
    /// A keeper was dispatched; the job can no longer be missed.
    InProgress,
    Completed,
    Missed,
}

/// One authoritative scheduled care occurrence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CareJob {
    pub id: u32,
    pub habitat_id: u32,
    pub category: CareCategory,
    pub due_minute: u64,
    pub priority: u32,
    pub required_keepers: u32,
    pub work_minutes: u32,
    pub state: CareJobState,
    /// The reserved crew, nearest first; empty until dispatch. The first keeper performs the
    /// care work, any other manages the animals.
    pub keeper_ids: Vec<u32>,
    /// The job's hold on supplies: reserved at dispatch, collected by the worker, consumed on
    /// completion, released when the job is interrupted.
    pub supply: JobSupply,
    pub resolved_minute: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct CareTaskView {
    pub category: CareCategory,
    pub first_minute_of_day: u32,
    pub interval_minutes: u32,
    pub priority: u32,
    pub work_minutes: u32,
    pub customized: bool,
    pub next_due_in_minutes: u64,
}

#[derive(Serialize)]
pub(crate) struct CareJobView {
    pub id: u32,
    pub habitat_id: u32,
    pub category: CareCategory,
    pub due_day: u32,
    pub due_minute_of_day: u32,
    pub priority: u32,
    pub required_keepers: u32,
    pub work_minutes: u32,
    pub state: CareJobState,
    pub keeper_ids: Vec<u32>,
    /// Why a due job has no keeper yet, or why its crew cannot move; `None` otherwise.
    pub waiting_reason: Option<String>,
    /// Minutes of grace left while due; zero once dispatched or resolved.
    pub expires_in_minutes: u64,
    pub phase: CarePhase,
    pub supply: JobSupply,
    /// Minutes since the job fell due while it is open; zero once resolved.
    pub waited_minutes: u64,
    /// Tiles left to walk: the crew's longest remaining route once dispatched, the depot to
    /// gate walk while due; `None` without a route.
    pub route_tiles: Option<u32>,
    /// Where inside the habitat the care is performed.
    pub care_spot: Option<super::Position>,
}

/// The park's care backlog at a glance (#104).
#[derive(Serialize)]
pub(crate) struct CareBacklogView {
    /// Due and dispatched jobs.
    pub open: u32,
    pub queued: u32,
    pub blocked: u32,
    pub in_progress: u32,
    /// The longest any open job has waited since it fell due.
    pub oldest_wait_minutes: u64,
    /// Jobs completed and missed within the retained day of history.
    pub completed_recent: u32,
    pub missed_recent: u32,
    /// Feed crates on the depot shelf, earmarked for dispatched jobs, and in keepers' hands.
    pub feed_on_shelf: u32,
    pub feed_reserved: u32,
    pub feed_carried: u32,
    /// Due jobs the depot cannot supply right now.
    pub supply_shortages: u32,
}

impl GameState {
    /// Generates due occurrences, reconciles dispatched jobs with zone changes, expires
    /// undispatched lapsed jobs, advances keepers working inside habitats, dispatches free
    /// keepers to eligible jobs, and prunes old resolved history. Runs once per simulated
    /// minute, so a long advance and many short ones produce identical outcomes.
    pub(crate) fn advance_care_jobs(&mut self) {
        let now = self.absolute_minute();
        self.generate_care_jobs(now);
        self.reconcile_care_jobs(now);

        for job in &mut self.care_jobs {
            if job.state == CareJobState::Due && now >= job.due_minute + CARE_GRACE_MINUTES {
                job.state = CareJobState::Missed;
                job.resolved_minute = Some(now);
                self.pending_events.push(crate::session::DomainEvent {
                    tick: self.elapsed_minutes,
                    kind: "care_job_missed",
                    entity_id: Some(job.id),
                });
            }
        }

        for keeper_index in 0..self.keepers.len() {
            if let KeeperActivity::Working {
                job_id,
                remaining_minutes,
            } = self.keepers[keeper_index].activity
            {
                if remaining_minutes > 1 {
                    self.keepers[keeper_index].activity = KeeperActivity::Working {
                        job_id,
                        remaining_minutes: remaining_minutes - 1,
                    };
                } else {
                    self.complete_care_job(keeper_index, job_id, now);
                }
            }
        }

        let mut due: Vec<usize> = (0..self.care_jobs.len())
            .filter(|&index| self.care_jobs[index].state == CareJobState::Due)
            .collect();
        due.sort_by(|&left, &right| {
            let (left, right) = (&self.care_jobs[left], &self.care_jobs[right]);
            right
                .priority
                .cmp(&left.priority)
                .then(left.due_minute.cmp(&right.due_minute))
                .then(left.id.cmp(&right.id))
        });
        for index in due {
            self.dispatch_care_job(index);
        }

        self.care_jobs.retain(|job| {
            job.resolved_minute
                .is_none_or(|resolved| resolved + RESOLVED_JOB_RETENTION_MINUTES > now)
        });
    }

    fn generate_care_jobs(&mut self, now: u64) {
        for habitat_index in 0..self.habitats.len() {
            let habitat_id = self.habitats[habitat_index].id;
            let Some(schedule) = self.habitats[habitat_index].care_schedule.as_mut() else {
                continue;
            };
            for task in &mut schedule.tasks {
                while task.next_due_minute <= now {
                    let due_minute = task.next_due_minute;
                    task.next_due_minute += u64::from(task.interval_minutes);
                    let duplicate = self.care_jobs.iter().any(|job| {
                        job.habitat_id == habitat_id
                            && job.category == task.category
                            && job.due_minute == due_minute
                    });
                    if duplicate {
                        continue;
                    }
                    let id = self.next_care_job_id;
                    self.next_care_job_id = self.next_care_job_id.saturating_add(1);
                    self.care_jobs.push(CareJob {
                        id,
                        habitat_id,
                        category: task.category,
                        due_minute,
                        priority: task.priority,
                        required_keepers: schedule.required_keepers,
                        work_minutes: task.work_minutes,
                        state: CareJobState::Due,
                        keeper_ids: Vec::new(),
                        supply: JobSupply::None,
                        resolved_minute: None,
                    });
                }
            }
        }
    }

    /// Interrupts dispatched jobs whose crew no longer all work in the zone holding the
    /// habitat's service gate: a reassigned keeper never finishes another zone's work, and a
    /// gate moved out of the crew's zone hands the job back to the scheduler.
    fn reconcile_care_jobs(&mut self, now: u64) {
        let stale: Vec<u32> = self
            .care_jobs
            .iter()
            .filter(|job| job.state == CareJobState::InProgress)
            .filter(|job| {
                let zone = self
                    .habitats
                    .iter()
                    .find(|habitat| habitat.id == job.habitat_id)
                    .and_then(|habitat| self.habitat_zone(habitat));
                zone.is_none()
                    || job.keeper_ids.iter().any(|id| {
                        self.keepers
                            .iter()
                            .find(|keeper| keeper.id == *id)
                            .is_none_or(|keeper| keeper.zone_id != zone)
                    })
            })
            .map(|job| job.id)
            .collect();
        for job_id in stale {
            self.interrupt_care_job(job_id, now);
        }
    }

    /// Hands a dispatched job back: its crew is released to walk back to the depot, its stock
    /// reservation is released, and a crate already collected stays with its carrier, who
    /// returns it to the depot shelf. The job is due again while its grace hour lasts, else
    /// missed.
    pub(crate) fn interrupt_care_job(&mut self, job_id: u32, now: u64) {
        let Some(job) = self
            .care_jobs
            .iter_mut()
            .find(|job| job.id == job_id && job.state == CareJobState::InProgress)
        else {
            return;
        };
        let crew = std::mem::take(&mut job.keeper_ids);
        job.supply = JobSupply::None;
        let missed = now >= job.due_minute + CARE_GRACE_MINUTES;
        if missed {
            job.state = CareJobState::Missed;
            job.resolved_minute = Some(now);
        } else {
            job.state = CareJobState::Due;
        }
        for keeper in &mut self.keepers {
            if crew.contains(&keeper.id) && keeper.activity.job_id() == Some(job_id) {
                keeper.activity = KeeperActivity::ToDepot;
            }
        }
        self.emit("care_job_interrupted", Some(job_id));
        if missed {
            self.emit("care_job_missed", Some(job_id));
        }
    }

    /// Whether the scheduler may give this keeper a job of `category` in `zone_id` now,
    /// ignoring whether the keeper is busy.
    fn keeper_eligible(&self, keeper: &Keeper, zone_id: u32, category: CareCategory) -> bool {
        keeper.zone_id == Some(zone_id)
            && keeper.schedule.accepts(category)
            && keeper.schedule.on_shift(self.minute_of_day)
    }

    /// Free for a new job: at or returning to the depot with empty hands. A keeper walking a
    /// crate back from an interrupted job returns it first.
    fn keeper_free(keeper: &Keeper) -> bool {
        keeper.activity.is_free() && !keeper.carrying_feed
    }

    /// The supplies a job needs at dispatch: a feed crate when the animals are hungry, the
    /// cleaning supplies when the habitat is dirty. Inspection and enrichment need none yet.
    fn job_supply(&self, habitat: &super::Habitat, category: CareCategory) -> Supply {
        match category {
            CareCategory::Feeding
                if habitat.stocked() && habitat.food < FEED_DELIVERY_THRESHOLD =>
            {
                Supply::FeedCrate
            }
            CareCategory::Cleaning if habitat.cleanliness < 100 => Supply::CleaningFee,
            _ => Supply::None,
        }
    }

    /// Crates earmarked by dispatched jobs whose workers have not collected them yet.
    pub(crate) fn reserved_feed_crates(&self) -> u32 {
        self.care_jobs
            .iter()
            .filter(|job| {
                job.state == CareJobState::InProgress && job.supply == JobSupply::FeedReserved
            })
            .count() as u32
    }

    /// Crates in keepers' hands: on their way to a feeding or back to the depot shelf.
    pub(crate) fn carried_feed_crates(&self) -> u32 {
        self.keepers
            .iter()
            .filter(|keeper| keeper.carrying_feed)
            .count() as u32
    }

    /// Why a due job's supplies cannot be reserved right now, if so: every crate on the shelf
    /// is already earmarked, or the cash cannot cover one more cleaning on top of those
    /// already promised.
    fn supply_shortage(&self, supply: Supply) -> Option<&'static str> {
        match supply {
            Supply::FeedCrate if self.feed_crates <= self.reserved_feed_crates() => {
                Some("The animal-care depot is out of unreserved feed crates")
            }
            Supply::CleaningFee => {
                let promised = self
                    .care_jobs
                    .iter()
                    .filter(|job| {
                        job.state == CareJobState::InProgress && job.supply == JobSupply::Cleaning
                    })
                    .count() as i64;
                let fee = self.catalog.balance.habitat_care.cleaning_cents;
                (self.cash_cents < fee.saturating_mul(promised + 1))
                    .then_some("Not enough cash for cleaning supplies")
            }
            _ => None,
        }
    }

    /// Matches one due job to its crew: the free eligible keepers with the shortest walks
    /// (for a feeding job, via the depot to collect the crate), lowest id on ties, as many as
    /// the job requires. The whole crew and the job's supplies are reserved together, or
    /// nothing is. Undispatched jobs stay due until their grace hour lapses.
    fn dispatch_care_job(&mut self, job_index: usize) {
        let job = &self.care_jobs[job_index];
        let (job_id, category, crew_size) = (job.id, job.category, job.required_keepers as usize);
        let Some(habitat_index) = self.habitats.iter().position(|h| h.id == job.habitat_id) else {
            return;
        };
        let habitat = &self.habitats[habitat_index];
        let Some(zone_id) = self.habitat_zone(habitat) else {
            return;
        };
        // The park has a single depot, so the nearest reachable depot with unreserved stock
        // is that depot or none.
        let supply = self.job_supply(habitat, category);
        if self.supply_shortage(supply).is_some() {
            return;
        }
        let depot = self.depot_position();
        let to_gate = KeeperActivity::ToGate { job_id };
        // The crate's leg: from the depot shelf to the gate.
        let depot_leg = if supply == Supply::FeedCrate {
            self.count_path_search();
            let Some(leg) = self.route_for(depot, to_gate, None) else {
                return;
            };
            Some(leg.len())
        } else {
            None
        };

        // (walk length, direct walk to the gate, keeper index): sorting picks the nearest,
        // lowest id on ties.
        let mut crew: Vec<(usize, usize, usize)> = Vec::new();
        for (keeper_index, keeper) in self.keepers.iter().enumerate() {
            if !Self::keeper_free(keeper) || !self.keeper_eligible(keeper, zone_id, category) {
                continue;
            }
            let here = Position {
                x: keeper.x,
                y: keeper.y,
            };
            self.count_path_search();
            let Some(direct) = self.route_for(here, to_gate, keeper.exit) else {
                continue;
            };
            let walk = match depot_leg {
                Some(leg) => {
                    self.count_path_search();
                    let collecting = KeeperActivity::CollectingStock { job_id };
                    let Some(to_depot) = self.route_for(here, collecting, keeper.exit) else {
                        continue;
                    };
                    to_depot.len() + leg
                }
                None => direct.len(),
            };
            crew.push((walk, direct.len(), keeper_index));
        }
        if crew.len() < crew_size {
            return;
        }
        crew.sort_unstable();
        crew.truncate(crew_size);

        let job = &mut self.care_jobs[job_index];
        job.state = CareJobState::InProgress;
        job.keeper_ids = crew
            .iter()
            .map(|&(_, _, keeper_index)| self.keepers[keeper_index].id)
            .collect();
        job.supply = match supply {
            Supply::FeedCrate => JobSupply::FeedReserved,
            Supply::CleaningFee => JobSupply::Cleaning,
            Supply::None => JobSupply::None,
        };
        for (order, &(_, direct, keeper_index)) in crew.iter().enumerate() {
            // Only the worker (the first of the crew) fetches the crate.
            self.keepers[keeper_index].activity = if order == 0 && supply == Supply::FeedCrate {
                KeeperActivity::CollectingStock { job_id }
            } else if direct <= 1 {
                // A keeper still inside after the previous job is already at this gate.
                KeeperActivity::AwaitingCrew { job_id }
            } else {
                to_gate
            };
        }
        let worker = crew[0].2;
        if supply == Supply::FeedCrate
            && (self.keepers[worker].x, self.keepers[worker].y) == (depot.x, depot.y)
        {
            self.collect_stock(worker, job_id);
        }
        self.start_when_crew_assembled(job_id);
    }

    /// The worker at the depot takes the job's reserved crate off the shelf and heads for the
    /// gate. The reservation guarantees the crate is there.
    fn collect_stock(&mut self, keeper_index: usize, job_id: u32) {
        let Some(job) = self
            .care_jobs
            .iter_mut()
            .find(|job| job.id == job_id && job.supply == JobSupply::FeedReserved)
        else {
            self.keepers[keeper_index].activity = KeeperActivity::ToGate { job_id };
            return;
        };
        if self.feed_crates == 0 {
            return;
        }
        self.feed_crates -= 1;
        job.supply = JobSupply::FeedCarried;
        let keeper = &mut self.keepers[keeper_index];
        keeper.carrying_feed = true;
        keeper.activity = KeeperActivity::ToGate { job_id };
    }

    /// Once every reserved keeper waits inside the gate, the crew walks to the care spot
    /// together.
    fn start_when_crew_assembled(&mut self, job_id: u32) {
        let Some(job) = self.care_jobs.iter().find(|job| job.id == job_id) else {
            return;
        };
        let crew: Vec<usize> = job
            .keeper_ids
            .iter()
            .filter_map(|&id| self.keepers.iter().position(|k| k.id == id))
            .collect();
        let waiting = KeeperActivity::AwaitingCrew { job_id };
        if crew
            .iter()
            .any(|&index| self.keepers[index].activity != waiting)
        {
            return;
        }
        for keeper_index in crew {
            self.keepers[keeper_index].activity = KeeperActivity::ToCareSpot { job_id };
        }
        self.begin_care_work(job_id);
    }

    /// The tile inside the job's habitat where its care is performed.
    pub(crate) fn job_care_spot(&self, job: &CareJob) -> Option<Position> {
        let habitat = self.habitats.iter().find(|h| h.id == job.habitat_id)?;
        Some(habitat.care_spot(job.category))
    }

    /// Once the whole crew stands on the care spot, the work starts.
    fn begin_care_work(&mut self, job_id: u32) {
        let Some(job) = self.care_jobs.iter().find(|job| job.id == job_id) else {
            return;
        };
        let Some(spot) = self.job_care_spot(job) else {
            return;
        };
        let work_minutes = job.work_minutes;
        let crew: Vec<usize> = job
            .keeper_ids
            .iter()
            .filter_map(|&id| self.keepers.iter().position(|k| k.id == id))
            .collect();
        let walking = KeeperActivity::ToCareSpot { job_id };
        let arrived = |keeper: &Keeper| {
            keeper.activity == walking && (keeper.x, keeper.y) == (spot.x, spot.y)
        };
        if crew.is_empty() || !crew.iter().all(|&index| arrived(&self.keepers[index])) {
            return;
        }
        for keeper_index in crew {
            self.keepers[keeper_index].activity = KeeperActivity::Working {
                job_id,
                remaining_minutes: work_minutes,
            };
        }
    }

    /// A crew works in lockstep, so every member finishes the same minute; the first to
    /// finish resolves the job and applies its single outcome, consuming its supplies.
    fn complete_care_job(&mut self, keeper_index: usize, job_id: u32, now: u64) {
        self.keepers[keeper_index].activity = KeeperActivity::ToDepot;
        let Some(job) = self
            .care_jobs
            .iter_mut()
            .find(|job| job.id == job_id && job.state == CareJobState::InProgress)
        else {
            return;
        };
        job.state = CareJobState::Completed;
        job.resolved_minute = Some(now);
        let (habitat_id, category, supply, worker_id) = (
            job.habitat_id,
            job.category,
            job.supply,
            job.keeper_ids.first().copied(),
        );
        let cleaning_cents = self.catalog.balance.habitat_care.cleaning_cents;
        match (category, supply) {
            (CareCategory::Feeding, JobSupply::FeedCarried) => {
                if let Some(worker) = self.keepers.iter_mut().find(|k| Some(k.id) == worker_id) {
                    worker.carrying_feed = false;
                    worker.deliveries_completed = worker.deliveries_completed.saturating_add(1);
                }
                if let Some(habitat) = self.habitats.iter_mut().find(|h| h.id == habitat_id) {
                    habitat.food = 100;
                }
            }
            (CareCategory::Cleaning, JobSupply::Cleaning | JobSupply::CleaningPrepaid) => {
                if supply == JobSupply::Cleaning {
                    self.book_service_cost(cleaning_cents, ExpenseCategory::HabitatCare);
                }
                if let Some(habitat) = self.habitats.iter_mut().find(|h| h.id == habitat_id) {
                    habitat.cleanliness = 100;
                }
            }
            _ => {}
        }
    }

    /// Books the supplies of work already done, exactly once. Dispatch reserved the cash, but
    /// wages may have spent it since; the cost is then carried as debt, like wages.
    fn book_service_cost(&mut self, cents: i64, category: ExpenseCategory) {
        if self.spend(cents, category).is_ok() {
            return;
        }
        let Some(cash) = self
            .cash_cents
            .checked_sub(cents)
            .filter(|cash| (-content::MAX_CENTS..=content::MAX_CENTS).contains(cash))
        else {
            return;
        };
        let mut ledger = self.finance_today;
        if ledger.record_expense(category, cents) {
            self.cash_cents = cash;
            self.finance_today = ledger;
        }
    }

    /// The keeper's remaining walk: its own tile first, its destination last (the depot, the
    /// inside gate tile or the care spot). `None` when no route exists. Never counts as
    /// simulation work, so snapshots can present it.
    pub(crate) fn keeper_route(&self, keeper: &Keeper) -> Option<Vec<Position>> {
        let here = Position {
            x: keeper.x,
            y: keeper.y,
        };
        self.route_for(here, keeper.activity, keeper.exit)
    }

    /// The walk a keeper at `here` doing `activity` has left; `exit` is the outside tile it
    /// entered its current habitat from, used when that habitat's gate cannot be found.
    fn route_for(
        &self,
        here: Position,
        activity: KeeperActivity,
        exit: Option<Position>,
    ) -> Option<Vec<Position>> {
        let depot = self.depot_position();
        let job_habitat = |job_id: u32| {
            let job = self.care_jobs.iter().find(|j| j.id == job_id)?;
            let habitat = self.habitats.iter().find(|h| h.id == job.habitat_id)?;
            Some((job, habitat))
        };
        let (goal, destination) = match activity {
            KeeperActivity::AtDepot
            | KeeperActivity::AwaitingCrew { .. }
            | KeeperActivity::Working { .. } => return Some(vec![here]),
            KeeperActivity::ToCareSpot { job_id } => {
                let (job, habitat) = job_habitat(job_id)?;
                return habitat
                    .roam_area()
                    .path(here, habitat.care_spot(job.category));
            }
            KeeperActivity::ToGate { job_id } => {
                let (_, habitat) = job_habitat(job_id)?;
                let (inside, outside) = self.service_gate_tiles(habitat)?;
                if self.tile(here.x, here.y) == Some(TileKind::Habitat(habitat.id)) {
                    // Already inside: walk through the habitat back to its gate.
                    return habitat.roam_area().path(here, inside);
                }
                (Some(outside), inside)
            }
            KeeperActivity::CollectingStock { .. } | KeeperActivity::ToDepot if here == depot => {
                return Some(vec![here]);
            }
            KeeperActivity::CollectingStock { .. } | KeeperActivity::ToDepot => (None, depot),
        };
        let access = self.depot_access_tiles();
        let goals = goal.map_or_else(|| access.clone(), |goal| vec![goal]);
        let mut route = Vec::new();
        let walk = if here == depot {
            // Leave the depot through whichever access tile is nearest the gate.
            route.push(here);
            let mut walk = self.search_path(Mover::Staff, goal?, &access)?;
            walk.reverse();
            walk
        } else if self.is_walkable(here) {
            self.search_path(Mover::Staff, here, &goals)?
        } else if let Some(TileKind::Habitat(habitat_id)) = self.tile(here.x, here.y) {
            // Inside a habitat: walk back to its gate and leave through it (through the gap
            // the keeper came in by when the gate cannot be found).
            let habitat = self.habitats.iter().find(|h| h.id == habitat_id);
            let gate =
                habitat.and_then(|habitat| Some((habitat, self.service_gate_tiles(habitat)?)));
            let outside = match gate {
                Some((habitat, (inside, outside))) => {
                    let inner = habitat.roam_area().path(here, inside);
                    route.extend(inner.unwrap_or_else(|| vec![here]));
                    outside
                }
                None => {
                    route.push(here);
                    exit?
                }
            };
            self.search_path(Mover::Staff, outside, &goals)?
        } else {
            // The path under the keeper was demolished: step onto the neighbouring path tile
            // with the shortest onward walk.
            route.push(here);
            self.neighbors(here)
                .into_iter()
                .filter_map(|next| self.search_path(Mover::Staff, next, &goals))
                .min_by_key(Vec::len)?
        };
        route.extend(walk);
        route.push(destination);
        Some(route)
    }

    /// Walking keepers move one tile per staff movement step (three minutes). A crew member
    /// with no route waits where it is, and its job stays blocked; once the job's grace hour
    /// has lapsed the job is interrupted instead of waiting forever.
    pub(crate) fn advance_keeper_movement(&mut self) {
        let now = self.absolute_minute();
        for keeper_index in 0..self.keepers.len() {
            let keeper = &self.keepers[keeper_index];
            if !keeper.activity.is_walking() {
                continue;
            }
            self.count_path_search();
            let Some(route) = self.keeper_route(keeper) else {
                let lapsed = keeper.activity.job_id().filter(|&job_id| {
                    self.care_jobs
                        .iter()
                        .any(|job| job.id == job_id && now >= job.due_minute + CARE_GRACE_MINUTES)
                });
                if let Some(job_id) = lapsed {
                    self.interrupt_care_job(job_id, now);
                }
                continue;
            };
            if let Some(&next) = route.get(1) {
                let here = Position {
                    x: keeper.x,
                    y: keeper.y,
                };
                let entering = self.is_walkable(here)
                    && matches!(self.tile(next.x, next.y), Some(TileKind::Habitat(_)));
                let keeper = &mut self.keepers[keeper_index];
                if entering {
                    keeper.exit = Some(here);
                }
                keeper.x = next.x;
                keeper.y = next.y;
                if route.len() > 2 {
                    continue;
                }
            }
            self.keeper_arrived(keeper_index);
        }
        let dispatched: Vec<u32> = self
            .care_jobs
            .iter()
            .filter(|job| job.state == CareJobState::InProgress)
            .map(|job| job.id)
            .collect();
        for job_id in dispatched {
            self.begin_care_work(job_id);
        }
    }

    fn keeper_arrived(&mut self, keeper_index: usize) {
        let activity = self.keepers[keeper_index].activity;
        let job_exists =
            |state: &Self, job_id: u32| state.care_jobs.iter().any(|job| job.id == job_id);
        match activity {
            KeeperActivity::ToDepot => {
                let keeper = &mut self.keepers[keeper_index];
                keeper.exit = None;
                keeper.activity = KeeperActivity::AtDepot;
                // A crate from an interrupted job goes back on the shelf.
                if keeper.carrying_feed {
                    keeper.carrying_feed = false;
                    self.feed_crates = self.feed_crates.saturating_add(1);
                }
            }
            KeeperActivity::CollectingStock { job_id } if job_exists(self, job_id) => {
                self.collect_stock(keeper_index, job_id);
            }
            KeeperActivity::ToGate { job_id } if job_exists(self, job_id) => {
                self.keepers[keeper_index].activity = KeeperActivity::AwaitingCrew { job_id };
                self.start_when_crew_assembled(job_id);
            }
            KeeperActivity::ToCareSpot { job_id } if job_exists(self, job_id) => {}
            _ => self.keepers[keeper_index].activity = KeeperActivity::ToDepot,
        }
    }

    /// Demolishing a habitat cancels its care: reservations lapse with the jobs, keepers on
    /// its jobs or inside its former fence are back at the depot, and any crate they carried
    /// is back on the shelf.
    pub(crate) fn release_habitat_keepers(&mut self, habitat_id: u32) {
        let depot = self.depot_position();
        let mut returned = 0;
        for keeper_index in 0..self.keepers.len() {
            let keeper = &self.keepers[keeper_index];
            let on_habitat_job = keeper.activity.job_id().is_some_and(|job_id| {
                self.care_jobs
                    .iter()
                    .any(|job| job.id == job_id && job.habitat_id == habitat_id)
            });
            let here = Position {
                x: keeper.x,
                y: keeper.y,
            };
            let stranded = self.tile(here.x, here.y) == Some(TileKind::Habitat(habitat_id));
            let keeper = &mut self.keepers[keeper_index];
            if on_habitat_job || stranded {
                keeper.x = depot.x;
                keeper.y = depot.y;
                keeper.activity = KeeperActivity::AtDepot;
                keeper.exit = None;
                if keeper.carrying_feed {
                    keeper.carrying_feed = false;
                    returned += 1;
                }
            }
        }
        self.feed_crates = self.feed_crates.saturating_add(returned);
        self.care_jobs.retain(|job| job.habitat_id != habitat_id);
    }

    pub(crate) fn keeper_view(&self, keeper: &Keeper) -> super::KeeperView {
        let route = if keeper.activity.is_walking() {
            self.keeper_route(keeper)
        } else {
            Some(Vec::new())
        };
        let job = keeper
            .activity
            .job_id()
            .and_then(|job_id| self.care_jobs.iter().find(|job| job.id == job_id));
        let task = job.map_or_else(String::new, |job| format!("{:?}", job.category));
        let lower = task.to_lowercase();
        let habitat = job.map_or(0, |job| job.habitat_id);
        let performs_care = job.is_none_or(|job| job.keeper_ids.first() == Some(&keeper.id));
        let partner =
            job.and_then(|job| job.keeper_ids.iter().copied().find(|&id| id != keeper.id));
        let status = match (keeper.activity, &route) {
            (KeeperActivity::CollectingStock { .. }, None) => {
                "Blocked: no path to the depot to collect feed".to_owned()
            }
            (KeeperActivity::ToGate { .. }, None) => {
                format!("Blocked: no path to habitat #{habitat}'s service gate")
            }
            (KeeperActivity::ToCareSpot { .. }, None) => {
                format!("Blocked: cannot reach the {lower} spot inside habitat #{habitat}")
            }
            (KeeperActivity::ToDepot, None) => "Blocked: no path back to the depot".to_owned(),
            (KeeperActivity::CollectingStock { .. }, Some(_)) => {
                format!("Collecting a feed crate at the depot for habitat #{habitat}")
            }
            (KeeperActivity::ToGate { .. }, Some(_)) if keeper.carrying_feed => {
                format!("Carrying feed to habitat #{habitat}'s service gate")
            }
            (KeeperActivity::ToGate { .. }, Some(_)) => {
                format!("Walking to habitat #{habitat}'s service gate for {lower}")
            }
            (KeeperActivity::ToCareSpot { .. }, Some(_)) => {
                format!("Walking to the {lower} spot inside habitat #{habitat}")
            }
            (KeeperActivity::AwaitingCrew { .. }, _) => format!(
                "Waiting inside habitat #{habitat}'s gate for keeper #{} before {lower}",
                partner.unwrap_or_default(),
            ),
            (
                KeeperActivity::Working {
                    remaining_minutes, ..
                },
                _,
            ) => match partner {
                None => format!("{task} in habitat #{habitat} · {remaining_minutes} min left"),
                Some(partner) if performs_care => format!(
                    "{task} in habitat #{habitat} while keeper #{partner} manages the animals · {remaining_minutes} min left"
                ),
                Some(partner) => format!(
                    "Managing the animals in habitat #{habitat} while keeper #{partner} does {lower} · {remaining_minutes} min left",
                ),
            },
            (KeeperActivity::ToDepot, Some(_)) if keeper.carrying_feed => {
                "Returning an unused feed crate to the depot".to_owned()
            }
            (KeeperActivity::ToDepot, Some(_)) => "Returning to the depot".to_owned(),
            (KeeperActivity::AtDepot, _) => match keeper.zone_id {
                None => "Idle · needs a work zone".to_owned(),
                Some(_) if !keeper.schedule.on_shift(self.minute_of_day) => format!(
                    "Off shift until {}",
                    clock(keeper.schedule.shift_start_minute)
                ),
                Some(zone_id) => format!("Available in work zone #{zone_id}"),
            },
        };
        super::KeeperView {
            id: keeper.id,
            x: keeper.x,
            y: keeper.y,
            zone_id: keeper.zone_id,
            needs_zone: keeper.zone_id.is_none(),
            categories: keeper.schedule.categories.clone(),
            shift_start_minute: keeper.schedule.shift_start_minute,
            shift_end_minute: keeper.schedule.shift_end_minute,
            on_shift: keeper.schedule.on_shift(self.minute_of_day),
            deliveries_completed: keeper.deliveries_completed,
            activity: keeper.activity.key(),
            job_id: keeper.activity.job_id(),
            habitat_id: job.map(|job| job.habitat_id),
            carrying_feed: keeper.carrying_feed,
            route: route.unwrap_or_default(),
            status,
        }
    }

    /// Sets a keeper's accepted categories (comma-separated keys) and daily availability
    /// window. Only future dispatch changes; a job already under way runs to completion.
    pub(crate) fn set_keeper_schedule(
        &mut self,
        keeper_id: u32,
        categories: &str,
        shift_start_minute: u32,
        shift_end_minute: u32,
    ) -> ActionResult {
        let mut accepted = Vec::new();
        for key in categories
            .split(',')
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            let Some(category) = CareCategory::parse(key) else {
                return ActionResult::error(format!("Unknown care category {key}"));
            };
            accepted.push(category);
        }
        if accepted.is_empty() {
            return ActionResult::error("A keeper must accept at least one care category");
        }
        accepted.sort();
        accepted.dedup();
        let day = MINUTES_PER_DAY as u32;
        if shift_start_minute >= day
            || shift_end_minute == 0
            || shift_end_minute > day
            || shift_start_minute == shift_end_minute % day && shift_end_minute != day
        {
            return ActionResult::error(
                "A shift starts within the day and ends after it starts (or wraps past midnight)",
            );
        }
        let Some(keeper) = self.keepers.iter_mut().find(|k| k.id == keeper_id) else {
            return ActionResult::error(format!("Keeper #{keeper_id} does not exist"));
        };
        let schedule = KeeperSchedule {
            categories: accepted,
            shift_start_minute,
            shift_end_minute,
        };
        if keeper.schedule == schedule {
            return ActionResult::ok(format!("Keeper #{keeper_id} already works this schedule"));
        }
        keeper.schedule = schedule;
        ActionResult::ok(format!("Keeper #{keeper_id} schedule updated"))
    }

    /// Why a due job has no keeper yet, so short staffing is visible on the job itself. Crew
    /// jobs say how many of their required keepers the zone can field. Problems the player
    /// must fix are `blocked`; keepers that are merely busy or off shift leave it queued.
    pub(crate) fn care_wait(&self, job: &CareJob) -> Option<CareWait> {
        use crate::alerts::AlertAction;
        if job.state != CareJobState::Due {
            return None;
        }
        let blocked = |message: String, action| {
            Some(CareWait {
                message,
                blocked: true,
                action,
                supply_shortage: false,
            })
        };
        let queued = |message: String| {
            Some(CareWait {
                message,
                blocked: false,
                action: AlertAction::HireKeeper,
                supply_shortage: false,
            })
        };
        let habitat = self.habitats.iter().find(|h| h.id == job.habitat_id)?;
        let Some(zone_id) = self.habitat_zone(habitat) else {
            let problem = self.zone_staffing_problem(habitat)?;
            return blocked(problem, AlertAction::DrawZone);
        };
        if let Some(problem) = self.zone_staffing_problem(habitat) {
            let action = if self.keepers.is_empty() {
                AlertAction::HireKeeper
            } else {
                AlertAction::AssignKeeper
            };
            return blocked(problem, action);
        }
        let category = job.category.key();
        let need = job.required_keepers as usize;
        let short = |count: usize, what: &str| {
            format!(
                "Needs {need} keepers together; only {count} zone #{zone_id} {category} keeper {what}"
            )
        };
        let zone_keepers = || self.keepers.iter().filter(|k| k.zone_id == Some(zone_id));
        let accepting = zone_keepers()
            .filter(|k| k.schedule.accepts(job.category))
            .count();
        if accepting == 0 {
            return blocked(
                format!("No zone #{zone_id} keeper accepts {category}"),
                AlertAction::AssignKeeper,
            );
        }
        if accepting < need {
            return blocked(short(accepting, "exists"), AlertAction::AssignKeeper);
        }
        let eligible = || zone_keepers().filter(|k| self.keeper_eligible(k, zone_id, job.category));
        let on_shift = eligible().count();
        if on_shift == 0 {
            return queued(format!("No zone #{zone_id} {category} keeper is on shift"));
        }
        if on_shift < need {
            return queued(short(on_shift, "is on shift"));
        }
        if let Some(shortage) = self.supply_shortage(self.job_supply(habitat, job.category)) {
            let action = if job.category == CareCategory::Feeding {
                AlertAction::BuyFeed
            } else {
                AlertAction::InspectHabitat
            };
            return Some(CareWait {
                message: shortage.to_owned(),
                blocked: true,
                action,
                supply_shortage: true,
            });
        }
        if !self.service_gate_reachable(habitat) {
            return blocked(super::GATE_UNREACHABLE.to_owned(), AlertAction::BuildPath);
        }
        let free = eligible().filter(|k| Self::keeper_free(k)).count();
        if free >= need {
            return blocked(
                "A free keeper has no route to the service gate".to_owned(),
                AlertAction::BuildPath,
            );
        }
        if free == 0 {
            return queued(format!("All zone #{zone_id} {category} keepers are busy"));
        }
        queued(short(free, "is free"))
    }

    /// Where a job stands, and for a dispatched job whose crew cannot move, why.
    fn care_phase(&self, job: &CareJob) -> (CarePhase, Option<String>) {
        match job.state {
            CareJobState::Completed => return (CarePhase::Completed, None),
            CareJobState::Missed => return (CarePhase::Missed, None),
            CareJobState::Due => {
                return match self.care_wait(job) {
                    Some(wait) if wait.blocked => (CarePhase::Blocked, Some(wait.message)),
                    wait => (CarePhase::Queued, wait.map(|wait| wait.message)),
                };
            }
            CareJobState::InProgress => {}
        }
        let crew: Vec<&Keeper> = job
            .keeper_ids
            .iter()
            .filter_map(|id| self.keepers.iter().find(|k| k.id == *id))
            .collect();
        if let Some(stuck) = crew
            .iter()
            .find(|keeper| keeper.activity.is_walking() && self.keeper_route(keeper).is_none())
        {
            return (CarePhase::Blocked, Some(self.keeper_view(stuck).status));
        }
        let phase = match crew.first().map(|worker| worker.activity) {
            Some(KeeperActivity::CollectingStock { .. }) => CarePhase::CollectingStock,
            _ if crew
                .iter()
                .all(|k| matches!(k.activity, KeeperActivity::Working { .. })) =>
            {
                CarePhase::Servicing
            }
            Some(KeeperActivity::AwaitingCrew { .. }) => CarePhase::Reserved,
            _ => CarePhase::Traveling,
        };
        (phase, None)
    }

    /// Overrides one task of a habitat's own schedule; the catalog and other habitats are
    /// untouched. The next occurrence is recomputed from now without re-issuing a job for an
    /// occurrence that already exists.
    pub(crate) fn set_care_task(
        &mut self,
        habitat_id: u32,
        category: &str,
        first_minute_of_day: u32,
        interval_minutes: u32,
        priority: u32,
    ) -> ActionResult {
        let Some(category) = CareCategory::parse(category) else {
            return ActionResult::error("Unknown care category");
        };
        if !content::valid_care_timing(first_minute_of_day, interval_minutes) {
            return ActionResult::error(
                "Care must start within the day and repeat every 1–24 hours in steps that divide the day",
            );
        }
        if !(1..=content::MAX_CARE_PRIORITY).contains(&priority) {
            return ActionResult::error(format!(
                "Care priority must be 1–{}",
                content::MAX_CARE_PRIORITY
            ));
        }
        let now = self.absolute_minute();
        let Some(habitat) = self.habitats.iter_mut().find(|h| h.id == habitat_id) else {
            return ActionResult::error("Select a habitat first");
        };
        let Some(schedule) = habitat.care_schedule.as_mut() else {
            return ActionResult::error("Adopt animals to start this habitat's care schedule");
        };
        let Some(task) = schedule.tasks.iter_mut().find(|t| t.category == category) else {
            return ActionResult::error("This habitat's species has no such care task");
        };
        if task.first_minute_of_day == first_minute_of_day
            && task.interval_minutes == interval_minutes
            && task.priority == priority
        {
            return ActionResult::ok("Care task already uses this schedule");
        }
        task.first_minute_of_day = first_minute_of_day;
        task.interval_minutes = interval_minutes;
        task.priority = priority;
        task.customized = true;
        task.next_due_minute = next_occurrence(first_minute_of_day, interval_minutes, now);
        for job in &mut self.care_jobs {
            if job.habitat_id == habitat_id
                && job.category == category
                && job.state == CareJobState::Due
            {
                job.priority = priority;
            }
        }
        ActionResult::ok(format!("Habitat #{habitat_id} care schedule updated"))
    }

    pub(crate) fn care_job_views(&self, habitat_id: u32) -> Vec<CareJobView> {
        let now = self.absolute_minute();
        self.care_jobs
            .iter()
            .filter(|job| job.habitat_id == habitat_id)
            .map(|job| {
                let (phase, waiting_reason) = self.care_phase(job);
                let open = matches!(job.state, CareJobState::Due | CareJobState::InProgress);
                CareJobView {
                    id: job.id,
                    habitat_id: job.habitat_id,
                    category: job.category,
                    due_day: u32::try_from(job.due_minute / MINUTES_PER_DAY + 1)
                        .unwrap_or(u32::MAX),
                    due_minute_of_day: (job.due_minute % MINUTES_PER_DAY) as u32,
                    priority: job.priority,
                    required_keepers: job.required_keepers,
                    work_minutes: job.work_minutes,
                    state: job.state,
                    keeper_ids: job.keeper_ids.clone(),
                    waiting_reason,
                    expires_in_minutes: if job.state == CareJobState::Due {
                        (job.due_minute + CARE_GRACE_MINUTES).saturating_sub(now)
                    } else {
                        0
                    },
                    phase,
                    supply: job.supply,
                    waited_minutes: if open {
                        now.saturating_sub(job.due_minute)
                    } else {
                        0
                    },
                    route_tiles: if open {
                        self.job_route_tiles(job)
                    } else {
                        None
                    },
                    care_spot: self.job_care_spot(job),
                }
            })
            .collect()
    }

    /// Tiles left on the crew's longest current walk once dispatched; while due, the walk from
    /// the depot to the gate.
    fn job_route_tiles(&self, job: &CareJob) -> Option<u32> {
        let tiles = |route: Vec<Position>| u32::try_from(route.len().saturating_sub(1)).ok();
        if job.state == CareJobState::Due {
            let walk = self.route_for(
                self.depot_position(),
                KeeperActivity::ToGate { job_id: job.id },
                None,
            )?;
            return tiles(walk);
        }
        let mut longest = 0;
        for id in &job.keeper_ids {
            let keeper = self.keepers.iter().find(|k| k.id == *id)?;
            longest = longest.max(tiles(self.keeper_route(keeper)?)?);
        }
        Some(longest)
    }

    /// The park's open care work, its oldest wait, recent outcomes and where the feed is.
    pub(crate) fn care_backlog_view(&self) -> CareBacklogView {
        let now = self.absolute_minute();
        let mut view = CareBacklogView {
            open: 0,
            queued: 0,
            blocked: 0,
            in_progress: 0,
            oldest_wait_minutes: 0,
            completed_recent: 0,
            missed_recent: 0,
            feed_on_shelf: self.feed_crates,
            feed_reserved: self.reserved_feed_crates(),
            feed_carried: self.carried_feed_crates(),
            supply_shortages: 0,
        };
        for job in &self.care_jobs {
            match job.state {
                CareJobState::Completed => view.completed_recent += 1,
                CareJobState::Missed => view.missed_recent += 1,
                CareJobState::Due | CareJobState::InProgress => {
                    view.open += 1;
                    view.oldest_wait_minutes = view
                        .oldest_wait_minutes
                        .max(now.saturating_sub(job.due_minute));
                    if job.state == CareJobState::InProgress {
                        view.in_progress += 1;
                    } else if let Some(wait) = self.care_wait(job) {
                        if wait.blocked {
                            view.blocked += 1;
                        } else {
                            view.queued += 1;
                        }
                        if wait.supply_shortage {
                            view.supply_shortages += 1;
                        }
                    } else {
                        view.queued += 1;
                    }
                }
            }
        }
        view
    }

    pub(crate) fn care_task_views(&self, schedule: &CareSchedule) -> Vec<CareTaskView> {
        let now = self.absolute_minute();
        schedule
            .tasks
            .iter()
            .map(|task| CareTaskView {
                category: task.category,
                first_minute_of_day: task.first_minute_of_day,
                interval_minutes: task.interval_minutes,
                priority: task.priority,
                work_minutes: task.work_minutes,
                customized: task.customized,
                next_due_in_minutes: task.next_due_minute.saturating_sub(now),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_occurrence_is_anchored_to_the_day() {
        assert_eq!(next_occurrence(420, 120, 540), 540);
        assert_eq!(next_occurrence(420, 120, 541), 660);
        assert_eq!(next_occurrence(600, 1440, 540), 600);
        assert_eq!(next_occurrence(600, 1440, 601), 1440 + 600);
        assert_eq!(next_occurrence(360, 360, 1439), 1440);
        assert_eq!(next_occurrence(0, 60, 0), 0);
    }

    #[test]
    fn catalog_rejects_invalid_care_defaults_with_field_paths() {
        let base: serde_json::Value = serde_json::from_str(content::DEFAULT_DOCUMENT).unwrap();
        for (pointer, value, path) in [
            (
                "/species/0/care/required_keepers",
                0.into(),
                "species/0/care/required_keepers",
            ),
            (
                "/species/0/care/required_keepers",
                3.into(),
                "species/0/care/required_keepers",
            ),
            (
                "/species/0/care/schedule/0/priority",
                6.into(),
                "species/0/care/schedule/0/priority",
            ),
            (
                "/species/0/care/schedule/0/work_minutes",
                0.into(),
                "species/0/care/schedule/0/work_minutes",
            ),
            (
                "/species/0/care/schedule/0/work_minutes",
                61.into(),
                "species/0/care/schedule/0/work_minutes",
            ),
            (
                "/species/0/care/schedule/0/interval_minutes",
                50.into(),
                "species/0/care/schedule/0",
            ),
            (
                "/species/0/care/schedule/0/interval_minutes",
                100.into(),
                "species/0/care/schedule/0",
            ),
            (
                "/species/0/care/schedule/0/first_minute_of_day",
                1440.into(),
                "species/0/care/schedule/0",
            ),
            (
                "/species/0/care/schedule/1/category",
                "feeding".into(),
                "species/0/care/schedule/1/category",
            ),
            (
                "/species/0/care/schedule",
                serde_json::json!([]),
                "species/0/care/schedule",
            ),
        ] {
            let mut document = base.clone();
            *document.pointer_mut(pointer).unwrap() = value;
            let error = content::Catalog::parse(&document.to_string()).unwrap_err();
            assert!(error.starts_with(path), "{pointer}: {error}");
        }
    }
}
