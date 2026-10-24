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
//! Keepers walk (#95): from the depot along paths to the habitat's service gate, one tile per
//! staff movement step, step inside, work for the task's `work_minutes`, then take the next
//! job or walk back. A dispatched job always runs to completion; supplies (a feed crate or
//! the cleaning fee) are taken at dispatch. Without a route to the gate, care is blocked.
use super::{
    ActionResult, ExpenseCategory, FEED_DELIVERY_THRESHOLD, GameState, Keeper, Position, TileKind,
    content::{self, CareCategory, CareProfile},
};
use serde::{Deserialize, Serialize};

/// What a keeper is doing; positions live on the keeper itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeeperActivity {
    AtDepot,
    ToGate {
        job_id: u32,
    },
    /// Inside the gate, waiting for the rest of the job's crew to arrive.
    AwaitingCrew {
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
            Self::ToGate { .. } => "to_gate",
            Self::AwaitingCrew { .. } => "awaiting_crew",
            Self::Working { .. } => "working",
            Self::ToDepot => "to_depot",
        }
    }

    /// Free keepers can take a new job: at the depot or on the way back to it.
    pub fn is_free(self) -> bool {
        matches!(self, Self::AtDepot | Self::ToDepot)
    }

    pub fn job_id(self) -> Option<u32> {
        match self {
            Self::ToGate { job_id }
            | Self::AwaitingCrew { job_id }
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
    /// A feed crate or cleaning fee taken at dispatch and applied on completion.
    pub supplies_reserved: bool,
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
    /// Why a due job has no keeper yet; `None` once dispatched or resolved.
    pub waiting_reason: Option<String>,
    /// Minutes of grace left while due; zero once dispatched or resolved.
    pub expires_in_minutes: u64,
}

impl GameState {
    /// Generates due occurrences, expires undispatched lapsed jobs, advances keepers working
    /// inside habitats, dispatches free keepers to eligible jobs, and prunes old resolved
    /// history. Runs once per simulated minute, so a long advance and many short ones produce
    /// identical outcomes.
    pub(crate) fn advance_care_jobs(&mut self) {
        let now = self.absolute_minute();
        self.generate_care_jobs(now);

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
                        supplies_reserved: false,
                        resolved_minute: None,
                    });
                }
            }
        }
    }

    /// Whether the scheduler may give this keeper a job of `category` in `zone_id` now,
    /// ignoring whether the keeper is busy.
    fn keeper_eligible(&self, keeper: &Keeper, zone_id: u32, category: CareCategory) -> bool {
        keeper.zone_id == Some(zone_id)
            && keeper.schedule.accepts(category)
            && keeper.schedule.on_shift(self.minute_of_day)
    }

    /// The supplies a job needs at dispatch: a feed crate when the animals are hungry, the
    /// cleaning fee when the habitat is dirty. Inspection and enrichment need none yet.
    fn job_supply(&self, habitat: &super::Habitat, category: CareCategory) -> Supply {
        match category {
            CareCategory::Feeding
                if habitat.animals > 0 && habitat.food < FEED_DELIVERY_THRESHOLD =>
            {
                Supply::FeedCrate
            }
            CareCategory::Cleaning if habitat.cleanliness < 100 => Supply::CleaningFee,
            _ => Supply::None,
        }
    }

    /// Why a due job's supplies cannot be reserved from the depot right now, if so.
    fn supply_shortage(&self, supply: Supply) -> Option<&'static str> {
        match supply {
            Supply::FeedCrate if self.feed_crates == 0 => {
                Some("The animal-care depot is out of feed crates")
            }
            Supply::CleaningFee
                if self.cash_cents < self.catalog.balance.habitat_care.cleaning_cents =>
            {
                Some("Not enough cash for cleaning supplies")
            }
            _ => None,
        }
    }

    /// Matches one due job to its crew: the free eligible keepers with the shortest walks to
    /// the gate (lowest id on ties), as many as the job requires. The whole crew and the
    /// job's depot supplies are reserved together, or nothing is. Undispatched jobs stay due
    /// until their grace hour lapses.
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
        // Only one depot exists, so the nearest depot with stock is that depot or none.
        let supply = self.job_supply(habitat, category);
        if self.supply_shortage(supply).is_some() {
            return;
        }

        // (walk length, keeper index): sorting picks the nearest, lowest id on ties.
        let mut crew: Vec<(usize, usize)> = Vec::new();
        for (keeper_index, keeper) in self.keepers.iter().enumerate() {
            if !keeper.activity.is_free() || !self.keeper_eligible(keeper, zone_id, category) {
                continue;
            }
            let candidate = Keeper {
                activity: KeeperActivity::ToGate { job_id },
                ..keeper.clone()
            };
            self.count_path_search();
            if let Some(route) = self.keeper_route(&candidate) {
                crew.push((route.len(), keeper_index));
            }
        }
        if crew.len() < crew_size {
            return;
        }
        crew.sort_unstable();
        crew.truncate(crew_size);

        let supplies_reserved = match supply {
            Supply::FeedCrate => {
                self.feed_crates -= 1;
                true
            }
            Supply::CleaningFee => {
                if self
                    .spend(
                        self.catalog.balance.habitat_care.cleaning_cents,
                        ExpenseCategory::HabitatCare,
                    )
                    .is_err()
                {
                    return;
                }
                true
            }
            Supply::None => false,
        };

        let job = &mut self.care_jobs[job_index];
        job.state = CareJobState::InProgress;
        job.keeper_ids = crew
            .iter()
            .map(|&(_, keeper_index)| self.keepers[keeper_index].id)
            .collect();
        job.supplies_reserved = supplies_reserved;
        for &(route_length, keeper_index) in &crew {
            // A keeper still inside after the previous job is already at this gate.
            self.keepers[keeper_index].activity = if route_length <= 1 {
                KeeperActivity::AwaitingCrew { job_id }
            } else {
                KeeperActivity::ToGate { job_id }
            };
        }
        self.start_when_crew_assembled(job_id);
    }

    /// Once every reserved keeper waits inside the gate, the whole crew starts working.
    fn start_when_crew_assembled(&mut self, job_id: u32) {
        let Some(job) = self.care_jobs.iter().find(|job| job.id == job_id) else {
            return;
        };
        let work_minutes = job.work_minutes;
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
            self.keepers[keeper_index].activity = KeeperActivity::Working {
                job_id,
                remaining_minutes: work_minutes,
            };
        }
    }

    /// A crew works in lockstep, so every member finishes the same minute; the first to
    /// finish resolves the job and applies its single outcome.
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
        let (habitat_id, category, worker_id) = (
            job.habitat_id,
            job.category,
            job.keeper_ids.first().copied(),
        );
        if !job.supplies_reserved {
            return;
        }
        let Some(habitat) = self.habitats.iter_mut().find(|h| h.id == habitat_id) else {
            return;
        };
        match category {
            CareCategory::Feeding => {
                habitat.food = 100;
                if let Some(worker) = self.keepers.iter_mut().find(|k| Some(k.id) == worker_id) {
                    worker.deliveries_completed = worker.deliveries_completed.saturating_add(1);
                }
            }
            CareCategory::Cleaning => habitat.cleanliness = 100,
            CareCategory::Inspection | CareCategory::Enrichment => {}
        }
    }

    /// The keeper's remaining walk: its own tile first, its destination last (the inside gate
    /// tile or the depot). `None` when no route exists. Never counts as simulation work, so
    /// snapshots can present it.
    pub(crate) fn keeper_route(&self, keeper: &Keeper) -> Option<Vec<Position>> {
        let here = Position {
            x: keeper.x,
            y: keeper.y,
        };
        let depot = self.depot_position();
        let (goal, destination) = match keeper.activity {
            KeeperActivity::AtDepot
            | KeeperActivity::AwaitingCrew { .. }
            | KeeperActivity::Working { .. } => return Some(vec![here]),
            KeeperActivity::ToGate { job_id } => {
                let habitat_id = self.care_jobs.iter().find(|j| j.id == job_id)?.habitat_id;
                let habitat = self.habitats.iter().find(|h| h.id == habitat_id)?;
                let (inside, outside) = self.service_gate_tiles(habitat)?;
                if here == inside {
                    return Some(vec![here]);
                }
                (Some(outside), inside)
            }
            KeeperActivity::ToDepot if here == depot => return Some(vec![here]),
            KeeperActivity::ToDepot => (None, depot),
        };
        let access = self.depot_access_tiles();
        let goals = goal.map_or_else(|| access.clone(), |goal| vec![goal]);
        let mut route = Vec::new();
        let walk = if here == depot {
            // Leave the depot through whichever access tile is nearest the gate.
            route.push(here);
            let mut walk = self.search_path(goal?, &access)?;
            walk.reverse();
            walk
        } else if self.is_walkable(here) {
            self.search_path(here, &goals)?
        } else if matches!(self.tile(here.x, here.y), Some(TileKind::Habitat(_))) {
            // Inside a habitat: leave through the gap the keeper came in by.
            route.push(here);
            self.search_path(keeper.exit?, &goals)?
        } else {
            // The path under the keeper was demolished: step onto the neighbouring path tile
            // with the shortest onward walk.
            route.push(here);
            self.neighbors(here)
                .into_iter()
                .filter_map(|next| self.search_path(next, &goals))
                .min_by_key(Vec::len)?
        };
        route.extend(walk);
        route.push(destination);
        Some(route)
    }

    /// Walking keepers move one tile per staff movement step (three minutes).
    pub(crate) fn advance_keeper_movement(&mut self) {
        for keeper_index in 0..self.keepers.len() {
            let keeper = &self.keepers[keeper_index];
            if !matches!(
                keeper.activity,
                KeeperActivity::ToGate { .. } | KeeperActivity::ToDepot
            ) {
                continue;
            }
            self.count_path_search();
            // Without a route the keeper waits where it is; care stays blocked meanwhile.
            let Some(route) = self.keeper_route(keeper) else {
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
    }

    fn keeper_arrived(&mut self, keeper_index: usize) {
        let keeper = &mut self.keepers[keeper_index];
        let KeeperActivity::ToGate { job_id } = keeper.activity else {
            keeper.exit = None;
            keeper.activity = KeeperActivity::AtDepot;
            return;
        };
        if !self.care_jobs.iter().any(|job| job.id == job_id) {
            self.keepers[keeper_index].activity = KeeperActivity::ToDepot;
            return;
        }
        self.keepers[keeper_index].activity = KeeperActivity::AwaitingCrew { job_id };
        self.start_when_crew_assembled(job_id);
    }

    /// Demolishing a habitat cancels its care: carried feed returns to the depot stock and
    /// keepers on its jobs or inside its former fence are back at the depot.
    pub(crate) fn release_habitat_keepers(&mut self, habitat_id: u32) {
        let depot = self.depot_position();
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
            }
        }
        let returned_crates = self
            .care_jobs
            .iter()
            .filter(|job| {
                job.habitat_id == habitat_id
                    && job.state == CareJobState::InProgress
                    && job.supplies_reserved
                    && job.category == CareCategory::Feeding
            })
            .count() as u32;
        self.feed_crates = self.feed_crates.saturating_add(returned_crates);
        self.care_jobs.retain(|job| job.habitat_id != habitat_id);
    }

    pub(crate) fn keeper_view(&self, keeper: &Keeper) -> super::KeeperView {
        let walking = matches!(
            keeper.activity,
            KeeperActivity::ToGate { .. } | KeeperActivity::ToDepot
        );
        let route = if walking {
            self.keeper_route(keeper)
        } else {
            Some(Vec::new())
        };
        let job = keeper
            .activity
            .job_id()
            .and_then(|job_id| self.care_jobs.iter().find(|job| job.id == job_id));
        let task = job.map_or_else(String::new, |job| format!("{:?}", job.category));
        let habitat = job.map_or(0, |job| job.habitat_id);
        let performs_care = job.is_none_or(|job| job.keeper_ids.first() == Some(&keeper.id));
        let partner =
            job.and_then(|job| job.keeper_ids.iter().copied().find(|&id| id != keeper.id));
        let status = match (keeper.activity, &route) {
            (KeeperActivity::ToGate { .. }, None) => {
                format!("Blocked: no path to habitat #{habitat}'s service gate")
            }
            (KeeperActivity::ToDepot, None) => "Blocked: no path back to the depot".to_owned(),
            (KeeperActivity::ToGate { .. }, Some(_)) => {
                format!(
                    "Walking to habitat #{habitat}'s service gate for {}",
                    task.to_lowercase()
                )
            }
            (KeeperActivity::AwaitingCrew { .. }, _) => format!(
                "Waiting inside habitat #{habitat}'s gate for keeper #{} before {}",
                partner.unwrap_or_default(),
                task.to_lowercase()
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
                    "Managing the animals in habitat #{habitat} while keeper #{partner} does {} · {remaining_minutes} min left",
                    task.to_lowercase()
                ),
            },
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
    /// jobs say how many of their required keepers the zone can field.
    fn waiting_reason(&self, job: &CareJob) -> Option<String> {
        if job.state != CareJobState::Due {
            return None;
        }
        let habitat = self.habitats.iter().find(|h| h.id == job.habitat_id)?;
        if let Some(problem) = self.zone_staffing_problem(habitat) {
            return Some(problem);
        }
        let zone_id = self.habitat_zone(habitat)?;
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
            return Some(format!("No zone #{zone_id} keeper accepts {category}"));
        }
        if accepting < need {
            return Some(short(accepting, "exists"));
        }
        let eligible = || zone_keepers().filter(|k| self.keeper_eligible(k, zone_id, job.category));
        let on_shift = eligible().count();
        if on_shift == 0 {
            return Some(format!("No zone #{zone_id} {category} keeper is on shift"));
        }
        if on_shift < need {
            return Some(short(on_shift, "is on shift"));
        }
        if let Some(shortage) = self.supply_shortage(self.job_supply(habitat, job.category)) {
            return Some(shortage.to_owned());
        }
        if !self.service_gate_reachable(habitat) {
            return Some(super::GATE_UNREACHABLE.to_owned());
        }
        let free = eligible().filter(|k| k.activity.is_free()).count();
        if free >= need {
            return Some("A free keeper has no route to the service gate".to_owned());
        }
        if free == 0 {
            return Some(format!("All zone #{zone_id} {category} keepers are busy"));
        }
        Some(short(free, "is free"))
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
            .map(|job| CareJobView {
                id: job.id,
                habitat_id: job.habitat_id,
                category: job.category,
                due_day: u32::try_from(job.due_minute / MINUTES_PER_DAY + 1).unwrap_or(u32::MAX),
                due_minute_of_day: (job.due_minute % MINUTES_PER_DAY) as u32,
                priority: job.priority,
                required_keepers: job.required_keepers,
                work_minutes: job.work_minutes,
                state: job.state,
                keeper_ids: job.keeper_ids.clone(),
                waiting_reason: self.waiting_reason(job),
                expires_in_minutes: if job.state == CareJobState::Due {
                    (job.due_minute + CARE_GRACE_MINUTES).saturating_sub(now)
                } else {
                    0
                },
            })
            .collect()
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
