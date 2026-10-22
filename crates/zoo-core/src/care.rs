//! Habitat-owned recurring care schedules and the stable jobs they generate (#133).
//!
//! A habitat copies its species' care defaults when first stocked and owns that schedule
//! afterwards. Each scheduled occurrence becomes exactly one job keyed by habitat, category
//! and due minute. A job stays eligible for one in-game hour after it is due, then expires as
//! missed. Until zone-based keeper matching lands (#134), the habitat's scheduled keeper
//! performs eligible jobs; jobs that need more keepers than that stay due and are missed.
use super::{
    ActionResult, CLEAN_HABITAT_COST, ExpenseCategory, FEED_DELIVERY_THRESHOLD, GameState,
    content::{self, CareCategory, CareProfile},
};
use serde::Serialize;

pub(crate) const CARE_GRACE_MINUTES: u64 = 60;
/// Resolved jobs stay visible for one in-game day, which bounds the job list.
const RESOLVED_JOB_RETENTION_MINUTES: u64 = 24 * 60;
const MINUTES_PER_DAY: u64 = 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CareTask {
    pub category: CareCategory,
    pub first_minute_of_day: u32,
    pub interval_minutes: u32,
    pub priority: u32,
    /// Whether the player changed this task away from its species default.
    pub customized: bool,
    #[serde(skip)]
    pub next_due_minute: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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

/// The first occurrence at or after `now` of a cadence anchored at `first_minute_of_day`.
fn next_occurrence(first_minute_of_day: u32, interval_minutes: u32, now: u64) -> u64 {
    let interval = u64::from(interval_minutes);
    let day_start = now - now % MINUTES_PER_DAY;
    let offset = u64::from(first_minute_of_day) % interval;
    let since = (now - day_start).saturating_sub(offset);
    day_start + offset + since.div_ceil(interval) * interval
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CareJobState {
    Due,
    Completed,
    Missed,
}

/// One authoritative scheduled care occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CareJob {
    pub id: u32,
    pub habitat_id: u32,
    pub category: CareCategory,
    pub due_minute: u64,
    pub priority: u32,
    pub required_keepers: u32,
    pub state: CareJobState,
    pub resolved_minute: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct CareTaskView {
    #[serde(flatten)]
    pub task: CareTask,
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
    pub state: CareJobState,
    /// Minutes of grace left while due; zero once resolved.
    pub expires_in_minutes: u64,
}

impl GameState {
    /// Generates due occurrences, expires lapsed jobs, lets the scheduled keeper perform
    /// eligible ones, and prunes old resolved history. Runs once per simulated minute, so a
    /// long advance and many short ones produce identical outcomes.
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
            if self.perform_care_job(self.care_jobs[index]) {
                self.care_jobs[index].state = CareJobState::Completed;
                self.care_jobs[index].resolved_minute = Some(now);
            }
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
                        state: CareJobState::Due,
                        resolved_minute: None,
                    });
                }
            }
        }
    }

    /// The interim performer: the habitat's single scheduled keeper completes a job on the
    /// spot when supplies allow. Returns whether the job completed.
    fn perform_care_job(&mut self, job: CareJob) -> bool {
        let Some(index) = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == job.habitat_id)
        else {
            return false;
        };
        let Some(keeper_id) = self.habitats[index].keeper_id else {
            return false;
        };
        // One keeper is scheduled per habitat today; multi-keeper jobs wait for #134/#136.
        if job.required_keepers > 1 {
            return false;
        }
        match job.category {
            CareCategory::Feeding => {
                let habitat = &self.habitats[index];
                if habitat.animals > 0 && habitat.food < FEED_DELIVERY_THRESHOLD {
                    if self.feed_crates == 0 {
                        return false;
                    }
                    self.feed_crates -= 1;
                    self.habitats[index].food = 100;
                    if let Some(keeper) = self.keepers.iter_mut().find(|k| k.id == keeper_id) {
                        keeper.deliveries_completed = keeper.deliveries_completed.saturating_add(1);
                    }
                }
            }
            CareCategory::Cleaning => {
                if self.habitats[index].cleanliness < 100 {
                    if self
                        .spend(CLEAN_HABITAT_COST, ExpenseCategory::HabitatCare)
                        .is_err()
                    {
                        return false;
                    }
                    self.habitats[index].cleanliness = 100;
                }
            }
            // No inspection or enrichment rules exist yet; the visit itself is the outcome.
            CareCategory::Inspection | CareCategory::Enrichment => {}
        }
        true
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
                state: job.state,
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
                task: *task,
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
