//! Education signs, keeper talks, guest learning, donations, conservation milestones and
//! scenario grants (#116).
//!
//! **Signs.** An education sign is a one-tile facility built beside a public path. It teaches
//! the species of the nearest stocked habitat within [`SIGN_REACH`] tiles (lowest ID on ties);
//! a sign with no such habitat is decorative and teaches nothing. A guest reads a sign by
//! standing on one of its public customer tiles, so a sign on a path guests cannot reach is
//! never read either.
//!
//! **Talks.** A habitat may have one daily keeper talk: a start time, a duration and a number
//! of places. Each occurrence is one `talk` care job, served through the ordinary keeper
//! pipeline: a free keeper of the habitat's work zone on shift walks to the gate and the care
//! spot, then talks for the duration. Guests viewing the habitat reserve a place while the talk
//! is open, up to the talk's places, until half the talk is over; then no one joins.
//! Attendees stay at the viewpoint until the talk ends. A guest who leaves the viewpoint gives
//! its place back; a talk no keeper starts within [`TALK_GRACE_MINUTES`], a talk whose habitat
//! empties, or a canceled talk releases every place without teaching anyone.
//!
//! **Learning.** A guest learns at most once per species: reading a sign gives
//! [`SIGN_POINTS`], attending a talk raises that species to [`TALK_POINTS`], and nothing repeats
//! (a second sign, the same sign rebuilt elsewhere, or another talk about a species already
//! learned at a talk adds nothing). A guest's learning is capped at [`MAX_LEARNING`]. What a
//! guest gains also lifts its happiness and value perception by a fifth of the points.
//!
//! **Donations.** A leaving guest who learned something and is content (happiness at least
//! [`DONATION_MIN_HAPPINESS`]) decides once to donate from the same spending money that paid
//! admission and stands: [`DONATION_CENTS_PER_POINT`] per learning point, plus a tenth for each
//! conservation milestone the park has earned (at most three), never more than the guest has
//! left. The gift is one ledger line, attributed to the habitat the guest learned most at. A
//! guest leaves exactly once, so no reload, repeated advance or relocated sign can repeat it.
//!
//! **Conservation milestones** are in-game achievements, each earned once with the animals and
//! event that earned it: a *thriving exhibit* (a habitat whose animals stay healthy at welfare
//! [`THRIVING_WELFARE`]+ for two days, naming the animals present the whole time), a *vet
//! recovery* (an animal that recovers after a vet's treatment) and a *supported rehoming* (an
//! animal that lived here at least [`REHOME_MIN_DAYS`] days, healthy and at welfare
//! [`REHOME_MIN_WELFARE`]+, moving to a partner zoo; a fresh adoption sent straight on never
//! counts). Breeding milestones wait for family life (#110). These are gameplay goals: they
//! make no claim about real-world conservation impact.
//!
//! **Grants** are scenario hooks: a scenario may promise a grant for an education or
//! conservation goal. Each park keeps its own copy; a grant pays once, through the ledger, the
//! minute its goal is met, and is remembered across saves.
use super::{
    ActionResult, GameState, GuestState, IncomeCategory, Position,
    care::{CareJob, CareJobState, JobSupply, KeeperActivity},
    content::{CareCategory, SpeciesFact},
    scenario::Goal,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Points a guest gains the first time it reads about a species.
pub(crate) const SIGN_POINTS: u32 = 10;
/// Points a species reaches once a guest has attended a talk about it.
pub(crate) const TALK_POINTS: u32 = 25;
/// The most a guest can learn in one visit.
pub(crate) const MAX_LEARNING: u32 = 100;
/// How far (in tiles, diagonals counting one) a sign may stand from the habitat it describes.
pub(crate) const SIGN_REACH: u32 = 2;
/// A talk no keeper has started this long after its start time is canceled.
pub(crate) const TALK_GRACE_MINUTES: u64 = 30;
/// Talks come after routine care when keepers are scarce.
const TALK_PRIORITY: u32 = 1;
pub(crate) const MIN_TALK_MINUTES: u32 = 10;
pub(crate) const MAX_TALK_MINUTES: u32 = 60;
pub(crate) const MAX_TALK_PLACES: u32 = 12;
/// Talks start on the quarter hour.
const TALK_START_STEP: u32 = 15;
pub(crate) const DONATION_CENTS_PER_POINT: i64 = 10;
pub(crate) const DONATION_MIN_HAPPINESS: u32 = 50;
const MILESTONE_DONATION_BONUS_PERCENT: i64 = 10;
const MAX_MILESTONE_BONUSES: i64 = 3;
pub(crate) const THRIVING_WELFARE: u32 = 80;
pub(crate) const THRIVING_MINUTES: u64 = 2 * MINUTES_PER_DAY;
pub(crate) const REHOME_MIN_DAYS: u32 = 3;
pub(crate) const REHOME_MIN_WELFARE: u32 = 60;
/// Recent talk outcomes and rehomings kept for the report.
const MAX_RECENT: usize = 12;
pub(crate) const MAX_GRANTS: usize = 8;
const MINUTES_PER_DAY: u64 = 24 * 60;

/// Where a guest learned about a species.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LessonSource {
    Sign,
    Talk,
}

/// What one guest learned about one species.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Lesson {
    pub species: String,
    /// The habitat the guest learned it at; donations are attributed to it.
    pub habitat_id: u32,
    pub source: LessonSource,
    pub points: u32,
}

/// A guest's learning points: the sum over its lessons, never above [`MAX_LEARNING`].
pub(crate) fn learning(lessons: &[Lesson]) -> u32 {
    lessons
        .iter()
        .fold(0_u32, |total, lesson| total.saturating_add(lesson.points))
        .min(MAX_LEARNING)
}

/// A habitat's daily keeper talk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TalkSchedule {
    pub habitat_id: u32,
    pub start_minute_of_day: u32,
    pub duration_minutes: u32,
    pub places: u32,
    /// The absolute minute of the next occurrence.
    pub next_due_minute: u64,
}

/// The guests holding places at one open talk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Attendance {
    pub job_id: u32,
    pub habitat_id: u32,
    pub places: u32,
    /// In the order they took their places; never more than `places`.
    pub guest_ids: Vec<u32>,
}

/// Education and donation counts for a day or for the park's whole life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Counts {
    /// Guests who learned their first lesson.
    pub guests_educated: u32,
    pub sign_lessons: u32,
    pub talk_lessons: u32,
    pub talks_given: u32,
    pub talks_canceled: u32,
    /// Guests who attended a talk to its end.
    pub attendees: u32,
    pub donors: u32,
    pub donations_cents: i64,
    pub grants_cents: i64,
}

/// Donations attributed to one habitat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HabitatDonations {
    pub habitat_id: u32,
    pub donors: u32,
    pub cents: i64,
}

/// How one talk ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TalkOutcome {
    pub habitat_id: u32,
    pub minute: u64,
    pub given: bool,
    pub attendees: u32,
    pub detail: String,
}

/// The park's conservation milestones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AchievementKind {
    ThrivingExhibit,
    VetRecovery,
    SupportedRehoming,
}

impl AchievementKind {
    pub const ALL: [Self; 3] = [
        Self::ThrivingExhibit,
        Self::VetRecovery,
        Self::SupportedRehoming,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::ThrivingExhibit => "Thriving exhibit",
            Self::VetRecovery => "Back on their feet",
            Self::SupportedRehoming => "Supported rehoming",
        }
    }

    fn description(self) -> String {
        match self {
            Self::ThrivingExhibit => format!(
                "Keep a habitat's animals healthy at welfare {THRIVING_WELFARE}+ for two days."
            ),
            Self::VetRecovery => "See an ill animal recover after a vet's treatment.".to_owned(),
            Self::SupportedRehoming => format!(
                "Send a healthy animal that lived here {REHOME_MIN_DAYS}+ days at welfare \
                 {REHOME_MIN_WELFARE}+ to a partner zoo."
            ),
        }
    }
}

/// An earned milestone and the real animals and event that earned it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Achievement {
    pub kind: AchievementKind,
    pub minute: u64,
    pub habitat_id: u32,
    pub animal_ids: Vec<u32>,
    pub detail: String,
}

/// One animal sent to a partner zoo, and whether it counted as a supported rehoming.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rehoming {
    pub animal_id: u32,
    pub minute: u64,
    pub eligible: bool,
    pub detail: String,
}

/// A habitat that has kept its animals healthy at thriving welfare since `since_minute`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WelfareStreak {
    pub habitat_id: u32,
    pub since_minute: u64,
}

/// A grant a scenario promises for an education or conservation goal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GrantDefinition {
    pub id: String,
    pub title: String,
    pub goal: Goal,
    pub amount_cents: i64,
}

/// A park's own copy of a grant; `awarded_minute` latches once it is paid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grant {
    pub id: String,
    pub title: String,
    pub goal: Goal,
    pub amount_cents: i64,
    pub awarded_minute: Option<u64>,
}

impl GrantDefinition {
    pub(crate) fn progress(&self) -> Grant {
        Grant {
            id: self.id.clone(),
            title: self.title.clone(),
            goal: self.goal.clone(),
            amount_cents: self.amount_cents,
            awarded_minute: None,
        }
    }
}

/// Everything the park remembers about education and conservation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Education {
    /// At most one per habitat, by habitat ID.
    pub talks: Vec<TalkSchedule>,
    /// One per open talk job, by job ID.
    pub attendance: Vec<Attendance>,
    pub today: Counts,
    pub total: Counts,
    /// By habitat ID.
    pub donations: Vec<HabitatDonations>,
    pub recent_talks: Vec<TalkOutcome>,
    pub achievements: Vec<Achievement>,
    /// Animals that recovered after a vet's treatment, each counted once, ascending.
    pub recovered_animals: Vec<u32>,
    pub supported_rehomings: u32,
    pub recent_rehomings: Vec<Rehoming>,
    pub streaks: Vec<WelfareStreak>,
    pub grants: Vec<Grant>,
}

impl Education {
    pub(crate) fn earned(&self, kind: AchievementKind) -> bool {
        self.achievements
            .iter()
            .any(|achievement| achievement.kind == kind)
    }

    /// Guests holding places at open talks.
    pub(crate) fn attending(&self) -> HashSet<u32> {
        self.attendance
            .iter()
            .flat_map(|attendance| attendance.guest_ids.iter().copied())
            .collect()
    }

    fn remember_talk(&mut self, outcome: TalkOutcome) {
        if outcome.given {
            self.today.talks_given = self.today.talks_given.saturating_add(1);
            self.total.talks_given = self.total.talks_given.saturating_add(1);
        } else {
            self.today.talks_canceled = self.today.talks_canceled.saturating_add(1);
            self.total.talks_canceled = self.total.talks_canceled.saturating_add(1);
        }
        self.recent_talks.push(outcome);
        let excess = self.recent_talks.len().saturating_sub(MAX_RECENT);
        self.recent_talks.drain(..excess);
    }
}

/// The first occurrence of a daily talk at or after `now`.
fn next_daily(start_minute_of_day: u32, now: u64) -> u64 {
    let day_start = now - now % MINUTES_PER_DAY;
    let today = day_start + u64::from(start_minute_of_day);
    if today >= now {
        today
    } else {
        today + MINUTES_PER_DAY
    }
}

fn clock(minute: u64) -> String {
    format!(
        "Day {} {:02}:{:02}",
        minute / MINUTES_PER_DAY + 1,
        minute % MINUTES_PER_DAY / 60,
        minute % 60
    )
}

// --- Views ------------------------------------------------------------------------------------

/// What a sign teaches, or why it teaches nothing.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SignView {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    /// The habitat it describes; `None` for a decorative sign.
    pub habitat_id: Option<u32>,
    pub species: Vec<String>,
    /// `teaching`, `decorative` or `unreachable`.
    pub status: &'static str,
    pub detail: String,
    /// The sourced facts it shows.
    pub facts: Vec<SpeciesFact>,
}

#[derive(Serialize)]
pub(crate) struct TalkView {
    pub habitat_id: u32,
    pub species: String,
    pub start_minute_of_day: u32,
    pub duration_minutes: u32,
    pub places: u32,
    pub next_due_minute: u64,
    /// `scheduled`, `waiting_for_keeper`, `keeper_on_the_way`, `in_progress` or `blocked`.
    pub status: &'static str,
    pub detail: String,
    pub attendees: u32,
    pub last: Option<TalkOutcome>,
}

#[derive(Serialize)]
pub(crate) struct LearningView {
    /// Guests in the park who have learned something, and their average learning.
    pub learners_now: u32,
    pub average_learning: u32,
    pub today: Counts,
    pub total: Counts,
}

#[derive(Serialize)]
pub(crate) struct DonationSourceView {
    pub habitat_id: u32,
    pub label: String,
    pub donors: u32,
    pub cents: i64,
}

#[derive(Serialize)]
pub(crate) struct AchievementView {
    pub kind: AchievementKind,
    pub title: &'static str,
    pub description: String,
    pub earned: Option<Achievement>,
    /// Where the park stands towards it, or how it was earned.
    pub progress: String,
}

#[derive(Serialize)]
pub(crate) struct GrantView {
    pub id: String,
    pub title: String,
    pub condition: String,
    pub amount_cents: i64,
    pub awarded_minute: Option<u64>,
    pub progress: String,
}

/// The conservation and education report.
#[derive(Serialize)]
pub(crate) struct EducationView {
    pub signs: Vec<SignView>,
    pub talks: Vec<TalkView>,
    pub learning: LearningView,
    pub donations: Vec<DonationSourceView>,
    pub recent_talks: Vec<TalkOutcome>,
    pub achievements: Vec<AchievementView>,
    pub recent_rehomings: Vec<Rehoming>,
    pub grants: Vec<GrantView>,
    /// The next useful step towards more learning, donations or milestones.
    pub reasons: Vec<String>,
}

impl Goal {
    /// Goals a grant may be promised for: counts the park can only raise through education and
    /// conservation events, so a grant can neither be met by spending nor un-met by demolition.
    pub(crate) fn education_goal(&self) -> bool {
        matches!(
            self,
            Goal::GuestsEducated { .. } | Goal::TalksGiven { .. } | Goal::Achievements { .. }
        )
    }

    fn describe(&self) -> String {
        match self {
            Goal::GuestsEducated { at_least } => {
                format!("{at_least} guests learn about the animals")
            }
            Goal::TalksGiven { at_least } => format!("{at_least} keeper talks are given"),
            Goal::Achievements { at_least } => {
                format!("{at_least} conservation milestones are earned")
            }
            _ => "A park goal is reached".to_owned(),
        }
    }
}

impl GameState {
    // --- Commands ---------------------------------------------------------------------------

    /// Schedules (or reschedules) a habitat's daily keeper talk.
    pub(crate) fn schedule_talk(
        &mut self,
        habitat_id: u32,
        start_minute_of_day: u32,
        duration_minutes: u32,
        places: u32,
    ) -> ActionResult {
        let Some(habitat) = self.habitats.iter().find(|h| h.id == habitat_id) else {
            return ActionResult::error(format!("Habitat #{habitat_id} does not exist"));
        };
        if habitat.quarantine {
            return ActionResult::error("Quarantine habitats are closed to talks");
        }
        if start_minute_of_day >= MINUTES_PER_DAY as u32
            || !start_minute_of_day.is_multiple_of(TALK_START_STEP)
        {
            return ActionResult::error("Talks start on the quarter hour between 00:00 and 23:45");
        }
        if !(MIN_TALK_MINUTES..=MAX_TALK_MINUTES).contains(&duration_minutes) {
            return ActionResult::error(format!(
                "A talk lasts {MIN_TALK_MINUTES} to {MAX_TALK_MINUTES} minutes"
            ));
        }
        if !(1..=MAX_TALK_PLACES).contains(&places) {
            return ActionResult::error(format!("A talk has 1 to {MAX_TALK_PLACES} places"));
        }
        if self.guest_viewpoint(habitat_id).is_none() {
            return ActionResult::error(format!(
                "Guests cannot reach a viewpoint at habitat #{habitat_id}: run a public path beside its fence"
            ));
        }
        let schedule = TalkSchedule {
            habitat_id,
            start_minute_of_day,
            duration_minutes,
            places,
            next_due_minute: next_daily(start_minute_of_day, self.absolute_minute()),
        };
        let talks = &mut self.education.talks;
        match talks.binary_search_by_key(&habitat_id, |talk| talk.habitat_id) {
            Ok(index) => talks[index] = schedule,
            Err(index) => talks.insert(index, schedule),
        }
        ActionResult::ok(format!(
            "Habitat #{habitat_id} keeper talk daily at {:02}:{:02} for {duration_minutes} min, {places} places",
            start_minute_of_day / 60,
            start_minute_of_day % 60
        ))
    }

    /// Cancels a habitat's talk schedule and any talk of it still open, releasing its keeper
    /// and every reserved place.
    pub(crate) fn cancel_talk(&mut self, habitat_id: u32) -> ActionResult {
        let Ok(index) = self
            .education
            .talks
            .binary_search_by_key(&habitat_id, |talk| talk.habitat_id)
        else {
            return ActionResult::error(format!("Habitat #{habitat_id} has no keeper talk"));
        };
        self.education.talks.remove(index);
        let open: Vec<u32> = self
            .care_jobs
            .iter()
            .filter(|job| {
                job.habitat_id == habitat_id
                    && job.category == CareCategory::Talk
                    && matches!(job.state, CareJobState::Due | CareJobState::InProgress)
            })
            .map(|job| job.id)
            .collect();
        for job_id in open {
            self.drop_talk(job_id, "Canceled by the park");
        }
        ActionResult::ok(format!("Habitat #{habitat_id} keeper talk canceled"))
    }

    /// Ends an open talk without teaching: its crew walks back, its places are released.
    fn drop_talk(&mut self, job_id: u32, detail: &str) {
        let now = self.absolute_minute();
        let Some(job) = self.care_jobs.iter_mut().find(|job| job.id == job_id) else {
            return;
        };
        let crew = std::mem::take(&mut job.keeper_ids);
        job.state = CareJobState::Missed;
        job.resolved_minute = Some(now);
        job.supply = JobSupply::None;
        let habitat_id = job.habitat_id;
        for keeper in &mut self.keepers {
            if crew.contains(&keeper.id) && keeper.activity.job_id() == Some(job_id) {
                keeper.activity = KeeperActivity::ToDepot;
            }
        }
        self.release_talk(job_id, habitat_id, detail);
    }

    fn release_talk(&mut self, job_id: u32, habitat_id: u32, detail: &str) {
        self.education
            .attendance
            .retain(|attendance| attendance.job_id != job_id);
        self.education.remember_talk(TalkOutcome {
            habitat_id,
            minute: self.absolute_minute(),
            given: false,
            attendees: 0,
            detail: detail.to_owned(),
        });
        self.emit("talk_canceled", Some(job_id));
    }

    /// Forgets a removed habitat's talk, open places and welfare streak.
    pub(crate) fn forget_habitat_education(&mut self, habitat_id: u32) {
        self.education
            .talks
            .retain(|talk| talk.habitat_id != habitat_id);
        self.education
            .attendance
            .retain(|attendance| attendance.habitat_id != habitat_id);
        self.education
            .streaks
            .retain(|streak| streak.habitat_id != habitat_id);
    }

    // --- Simulation -------------------------------------------------------------------------

    /// One talk job per scheduled occurrence; an occurrence while the habitat has no animals
    /// on show is canceled at once.
    pub(crate) fn generate_talk_jobs(&mut self, now: u64) {
        for index in 0..self.education.talks.len() {
            while self.education.talks[index].next_due_minute <= now {
                let talk = self.education.talks[index].clone();
                self.education.talks[index].next_due_minute += MINUTES_PER_DAY;
                let Some(habitat) = self.habitats.iter().find(|h| h.id == talk.habitat_id) else {
                    continue;
                };
                if !habitat.stocked() || habitat.quarantine {
                    self.education.remember_talk(TalkOutcome {
                        habitat_id: talk.habitat_id,
                        minute: talk.next_due_minute,
                        given: false,
                        attendees: 0,
                        detail: "No animals on show to talk about".to_owned(),
                    });
                    continue;
                }
                let required_keepers = habitat
                    .care_schedule
                    .as_ref()
                    .map_or(1, |schedule| schedule.required_keepers.max(1));
                let id = self.next_care_job_id;
                self.next_care_job_id = self.next_care_job_id.saturating_add(1);
                self.care_jobs.push(CareJob {
                    id,
                    habitat_id: talk.habitat_id,
                    category: CareCategory::Talk,
                    due_minute: talk.next_due_minute,
                    priority: TALK_PRIORITY,
                    required_keepers,
                    work_minutes: talk.duration_minutes,
                    state: CareJobState::Due,
                    keeper_ids: Vec::new(),
                    supply: JobSupply::None,
                    resolved_minute: None,
                    animal_id: None,
                });
                self.education.attendance.push(Attendance {
                    job_id: id,
                    habitat_id: talk.habitat_id,
                    places: talk.places,
                    guest_ids: Vec::new(),
                });
            }
        }
    }

    /// The keeper finished the talk: every guest still holding a place learns about each
    /// species on show.
    pub(crate) fn finish_talk(&mut self, job_id: u32, habitat_id: u32) {
        let Some(index) = self
            .education
            .attendance
            .iter()
            .position(|attendance| attendance.job_id == job_id)
        else {
            return;
        };
        let attendance = self.education.attendance.remove(index);
        let species = self
            .habitats
            .iter()
            .find(|h| h.id == habitat_id)
            .map(|habitat| habitat.species_keys())
            .unwrap_or_default();
        let mut attendees = 0_u32;
        for guest_id in attendance.guest_ids {
            let Some(guest_index) = self.guests.iter().position(|g| g.id == guest_id) else {
                continue;
            };
            if !self.at_habitat(guest_index, habitat_id) {
                continue;
            }
            attendees += 1;
            for key in &species {
                self.teach(guest_index, key, habitat_id, LessonSource::Talk);
            }
        }
        self.education.today.attendees = self.education.today.attendees.saturating_add(attendees);
        self.education.total.attendees = self.education.total.attendees.saturating_add(attendees);
        self.education.remember_talk(TalkOutcome {
            habitat_id,
            minute: self.absolute_minute(),
            given: true,
            attendees,
            detail: format!(
                "{attendees} guest{} listened to the end",
                if attendees == 1 { "" } else { "s" }
            ),
        });
        self.emit("talk_given", Some(job_id));
    }

    fn at_habitat(&self, guest_index: usize, habitat_id: u32) -> bool {
        let guest = &self.guests[guest_index];
        guest.state == GuestState::Viewing && guest.target_habitat == habitat_id
    }

    /// Teaches one species; returns the points gained.
    fn teach(
        &mut self,
        guest_index: usize,
        species: &str,
        habitat_id: u32,
        source: LessonSource,
    ) -> u32 {
        let target = match source {
            LessonSource::Sign => SIGN_POINTS,
            LessonSource::Talk => TALK_POINTS,
        };
        let guest = &mut self.guests[guest_index];
        let before = learning(&guest.lessons);
        let first = guest.lessons.is_empty();
        let known = guest
            .lessons
            .iter()
            .find(|lesson| lesson.species == species)
            .map_or(0, |lesson| lesson.points);
        let gain = target
            .saturating_sub(known)
            .min(MAX_LEARNING.saturating_sub(before));
        if gain == 0 {
            return 0;
        }
        match guest
            .lessons
            .iter_mut()
            .find(|lesson| lesson.species == species)
        {
            Some(lesson) => {
                lesson.points += gain;
                lesson.source = source;
                lesson.habitat_id = habitat_id;
            }
            None => guest.lessons.push(Lesson {
                species: species.to_owned(),
                habitat_id,
                source,
                points: gain,
            }),
        }
        guest.happiness = guest.happiness.saturating_add(gain / 5).min(100);
        guest.value_perception = guest.value_perception.saturating_add(gain / 5).min(100);
        let guest_id = guest.id;
        for counts in [&mut self.education.today, &mut self.education.total] {
            if first {
                counts.guests_educated = counts.guests_educated.saturating_add(1);
            }
            match source {
                LessonSource::Sign => counts.sign_lessons = counts.sign_lessons.saturating_add(1),
                LessonSource::Talk => counts.talk_lessons = counts.talk_lessons.saturating_add(1),
            }
        }
        self.emit("guest_learned", Some(guest_id));
        gain
    }

    /// Once a minute: talk places, sign reading, welfare streaks, milestones and grants.
    pub(crate) fn advance_education(&mut self) {
        self.reconcile_talks();
        self.read_signs();
        if self.minute_of_day.is_multiple_of(60) {
            self.advance_welfare_streaks();
        }
        self.award_grants();
    }

    /// Ends talks that can no longer be given and keeps each open talk's places honest:
    /// guests who left the viewpoint give theirs back, viewers take free ones until the keeper
    /// starts.
    fn reconcile_talks(&mut self) {
        for index in (0..self.education.attendance.len()).rev() {
            let (job_id, habitat_id) = {
                let attendance = &self.education.attendance[index];
                (attendance.job_id, attendance.habitat_id)
            };
            let job = self.care_jobs.iter().find(|job| job.id == job_id);
            match job.map(|job| job.state) {
                Some(CareJobState::Due | CareJobState::InProgress) => {}
                Some(CareJobState::Missed) => {
                    self.release_talk(
                        job_id,
                        habitat_id,
                        &format!("No keeper could start it within {TALK_GRACE_MINUTES} minutes"),
                    );
                    continue;
                }
                Some(CareJobState::Completed) | None => {
                    self.education.attendance.remove(index);
                    continue;
                }
            }
            let stocked = self
                .habitats
                .iter()
                .find(|h| h.id == habitat_id)
                .is_some_and(|habitat| habitat.stocked() && !habitat.quarantine);
            if !stocked {
                self.drop_talk(job_id, "The animals left the habitat");
                continue;
            }
            // Guests may join until half the talk is over.
            let duration = job.map_or(0, |job| job.work_minutes);
            let late = self.keepers.iter().any(|keeper| {
                matches!(
                    keeper.activity,
                    KeeperActivity::Working { job_id: id, remaining_minutes }
                        if id == job_id && remaining_minutes.saturating_mul(2) < duration
                )
            });
            let holding: Vec<u32> = self.education.attendance[index]
                .guest_ids
                .iter()
                .copied()
                .filter(|id| {
                    self.guests
                        .iter()
                        .position(|guest| guest.id == *id)
                        .is_some_and(|guest_index| self.at_habitat(guest_index, habitat_id))
                })
                .collect();
            let mut taken = holding;
            if !late {
                let places = self.education.attendance[index].places as usize;
                let elsewhere: HashSet<u32> = self
                    .education
                    .attendance
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .flat_map(|(_, attendance)| attendance.guest_ids.iter().copied())
                    .collect();
                for guest in &self.guests {
                    if taken.len() >= places {
                        break;
                    }
                    if guest.state == GuestState::Viewing
                        && guest.target_habitat == habitat_id
                        && !taken.contains(&guest.id)
                        && !elsewhere.contains(&guest.id)
                    {
                        taken.push(guest.id);
                    }
                }
            }
            self.education.attendance[index].guest_ids = taken;
        }
    }

    /// The habitat a sign describes and its species: the nearest stocked, ordinary habitat
    /// within [`SIGN_REACH`] tiles, lowest ID on ties.
    pub(crate) fn sign_subject(&self, at: Position) -> Option<(u32, Vec<String>)> {
        self.habitats
            .iter()
            .filter(|habitat| habitat.stocked() && !habitat.quarantine)
            .filter_map(|habitat| {
                let distance = habitat
                    .tiles
                    .iter()
                    .map(|tile| tile.x.abs_diff(at.x).max(tile.y.abs_diff(at.y)))
                    .min()?;
                (distance <= SIGN_REACH).then_some((distance, habitat.id, habitat))
            })
            .min_by_key(|(distance, id, _)| (*distance, *id))
            .map(|(_, id, habitat)| (id, habitat.species_keys()))
    }

    fn signs(&self) -> impl Iterator<Item = &super::Concession> {
        self.concessions
            .iter()
            .filter(|stand| stand.kind == super::ConcessionKind::Sign)
    }

    /// Guests standing at a teaching sign read it.
    fn read_signs(&mut self) {
        let mut readers: HashMap<(u32, u32), (u32, Vec<String>)> = HashMap::new();
        for sign in self.signs() {
            let Some(subject) = self.sign_subject(Position {
                x: sign.x,
                y: sign.y,
            }) else {
                continue;
            };
            for tile in self.concession_customer_tiles(sign.id) {
                readers.entry((tile.x, tile.y)).or_insert(subject.clone());
            }
        }
        if readers.is_empty() {
            return;
        }
        for guest_index in 0..self.guests.len() {
            let guest = &self.guests[guest_index];
            if guest.state == GuestState::Arriving {
                continue;
            }
            let Some((habitat_id, species)) = readers.get(&(guest.x, guest.y)).cloned() else {
                continue;
            };
            for key in &species {
                self.teach(guest_index, key, habitat_id, LessonSource::Sign);
            }
        }
    }

    /// Leaving guests who learned something decide once whether to donate.
    pub(crate) fn collect_donations(&mut self, leaving: &[u32]) {
        let milestones = i64::try_from(self.education.achievements.len())
            .unwrap_or(0)
            .min(MAX_MILESTONE_BONUSES);
        for guest_id in leaving {
            let Some(index) = self.guests.iter().position(|guest| guest.id == *guest_id) else {
                continue;
            };
            let guest = &self.guests[index];
            let points = learning(&guest.lessons);
            if points == 0 || guest.happiness < DONATION_MIN_HAPPINESS {
                continue;
            }
            let wish = i64::from(points)
                * DONATION_CENTS_PER_POINT
                * (100 + MILESTONE_DONATION_BONUS_PERCENT * milestones)
                / 100;
            let gift = wish.min(guest.cash_cents);
            if gift <= 0 {
                continue;
            }
            // The habitat the guest learned most at, earliest on ties.
            let Some(habitat_id) = guest
                .lessons
                .iter()
                .rev()
                .max_by_key(|lesson| lesson.points)
                .map(|lesson| lesson.habitat_id)
            else {
                continue;
            };
            if !self.earn(gift, IncomeCategory::Donations) {
                continue;
            }
            self.guests[index].cash_cents -= gift;
            for counts in [&mut self.education.today, &mut self.education.total] {
                counts.donors = counts.donors.saturating_add(1);
                counts.donations_cents = counts.donations_cents.saturating_add(gift);
            }
            let donations = &mut self.education.donations;
            let at = match donations.binary_search_by_key(&habitat_id, |entry| entry.habitat_id) {
                Ok(at) => at,
                Err(at) => {
                    donations.insert(
                        at,
                        HabitatDonations {
                            habitat_id,
                            donors: 0,
                            cents: 0,
                        },
                    );
                    at
                }
            };
            donations[at].donors = donations[at].donors.saturating_add(1);
            donations[at].cents = donations[at].cents.saturating_add(gift);
            self.emit("donation", Some(*guest_id));
        }
    }

    /// Hourly: which habitats keep healthy animals at thriving welfare, and whether one has
    /// done so for two days.
    fn advance_welfare_streaks(&mut self) {
        let now = self.absolute_minute();
        let thriving: Vec<u32> = self
            .habitats
            .iter()
            .filter(|habitat| {
                habitat.stocked()
                    && !habitat.quarantine
                    && habitat.welfare() >= THRIVING_WELFARE
                    && habitat.animals.iter().all(|animal| !animal.health.is_ill())
            })
            .map(|habitat| habitat.id)
            .collect();
        let streaks = &mut self.education.streaks;
        streaks.retain(|streak| thriving.contains(&streak.habitat_id));
        for habitat_id in &thriving {
            if let Err(at) = streaks.binary_search_by_key(habitat_id, |streak| streak.habitat_id) {
                streaks.insert(
                    at,
                    WelfareStreak {
                        habitat_id: *habitat_id,
                        since_minute: now,
                    },
                );
            }
        }
        if self.education.earned(AchievementKind::ThrivingExhibit) {
            return;
        }
        let candidate = self
            .education
            .streaks
            .iter()
            .filter(|streak| now >= streak.since_minute + THRIVING_MINUTES)
            .find_map(|streak| {
                let habitat = self.habitats.iter().find(|h| h.id == streak.habitat_id)?;
                // Only animals present for the whole streak earned it.
                let animals: Vec<&super::animal::Animal> = habitat
                    .animals
                    .iter()
                    .filter(|animal| {
                        u64::from(animal.arrived_day.saturating_sub(1)) * MINUTES_PER_DAY
                            <= streak.since_minute
                    })
                    .collect();
                (!animals.is_empty()).then(|| {
                    let names: Vec<String> = animals
                        .iter()
                        .map(|animal| format!("{} (#{})", animal.name, animal.id))
                        .collect();
                    Achievement {
                        kind: AchievementKind::ThrivingExhibit,
                        minute: now,
                        habitat_id: habitat.id,
                        animal_ids: animals.iter().map(|animal| animal.id).collect(),
                        detail: format!(
                            "Habitat #{} ({}) kept {} healthy at welfare {THRIVING_WELFARE}+ since {}",
                            habitat.id,
                            habitat.species_labels(),
                            names.join(", "),
                            clock(streak.since_minute)
                        ),
                    }
                })
            });
        if let Some(achievement) = candidate {
            self.award(achievement);
        }
    }

    fn award(&mut self, achievement: Achievement) {
        if self.education.earned(achievement.kind) {
            return;
        }
        let habitat_id = achievement.habitat_id;
        self.education.achievements.push(achievement);
        self.emit("achievement_earned", Some(habitat_id));
    }

    /// An animal recovered after a vet's treatment (#109); each animal counts once.
    pub(crate) fn note_recovery(&mut self, animal_id: u32) {
        let recovered = &mut self.education.recovered_animals;
        let Err(at) = recovered.binary_search(&animal_id) else {
            return;
        };
        recovered.insert(at, animal_id);
        let Some((index, position)) = self.locate_animal(animal_id) else {
            return;
        };
        let habitat = &self.habitats[index];
        let animal = &habitat.animals[position];
        // A patient recovers in quarantine; credit its home habitat.
        let home = animal.health.quarantine_home.unwrap_or(habitat.id);
        let achievement = Achievement {
            kind: AchievementKind::VetRecovery,
            minute: self.absolute_minute(),
            habitat_id: home,
            animal_ids: vec![animal_id],
            detail: format!(
                "{} (#{animal_id}) recovered after a vet's treatment",
                animal.name
            ),
        };
        self.award(achievement);
    }

    /// Records an animal about to leave for a partner zoo (#108) and whether it counts as a
    /// supported rehoming: it must have lived here [`REHOME_MIN_DAYS`] days, be healthy, and
    /// its species' welfare be at least [`REHOME_MIN_WELFARE`]. A fresh adoption never counts,
    /// so adopting and sending animals straight on earns nothing.
    pub(crate) fn note_rehoming(&mut self, animal_id: u32) {
        let Some((index, position)) = self.locate_animal(animal_id) else {
            return;
        };
        let habitat = &self.habitats[index];
        let animal = &habitat.animals[position];
        let days = self.day.saturating_sub(animal.arrived_day);
        let welfare = habitat
            .resident_index(&animal.species)
            .map_or(0, |resident| habitat.residents[resident].welfare);
        let name = format!("{} (#{animal_id})", animal.name);
        let problem = if days < REHOME_MIN_DAYS {
            Some(format!(
                "{name} had lived here {days} of {REHOME_MIN_DAYS} days: a new arrival does not count"
            ))
        } else if animal.health.is_ill() || habitat.quarantine {
            Some(format!("{name} was ill: only healthy animals count"))
        } else if welfare < REHOME_MIN_WELFARE {
            Some(format!(
                "{name} left at welfare {welfare}, below {REHOME_MIN_WELFARE}"
            ))
        } else {
            None
        };
        let eligible = problem.is_none();
        let detail = problem.unwrap_or_else(|| {
            format!("{name} moved to a partner zoo healthy at welfare {welfare} after {days} days")
        });
        let minute = self.absolute_minute();
        let habitat_id = habitat.id;
        let rehomings = &mut self.education.recent_rehomings;
        rehomings.push(Rehoming {
            animal_id,
            minute,
            eligible,
            detail: detail.clone(),
        });
        let excess = rehomings.len().saturating_sub(MAX_RECENT);
        rehomings.drain(..excess);
        if eligible {
            self.education.supported_rehomings =
                self.education.supported_rehomings.saturating_add(1);
            self.award(Achievement {
                kind: AchievementKind::SupportedRehoming,
                minute,
                habitat_id,
                animal_ids: vec![animal_id],
                detail,
            });
        }
    }

    /// Pays every grant whose goal has been met, once.
    fn award_grants(&mut self) {
        for index in 0..self.education.grants.len() {
            let grant = &self.education.grants[index];
            if grant.awarded_minute.is_some() || !self.goal_reached(&grant.goal) {
                continue;
            }
            let amount = grant.amount_cents;
            if !self.earn(amount, IncomeCategory::Grants) {
                continue;
            }
            self.education.grants[index].awarded_minute = Some(self.absolute_minute());
            self.education.today.grants_cents =
                self.education.today.grants_cents.saturating_add(amount);
            self.education.total.grants_cents =
                self.education.total.grants_cents.saturating_add(amount);
            self.emit("grant_awarded", u32::try_from(index).ok());
        }
    }

    // --- Views ------------------------------------------------------------------------------

    /// A public viewpoint tile beside the habitat that guests can reach from the entrance.
    fn guest_viewpoint(&self, habitat_id: u32) -> Option<Position> {
        let habitat = self.habitats.iter().find(|h| h.id == habitat_id)?;
        let reach = self.guest_reach();
        self.fence_outside_tiles(habitat)
            .into_iter()
            .map(|(position, _)| position)
            .find(|position| {
                self.walkable(super::Mover::Guest, *position) && reach.contains(self, *position)
            })
    }

    fn facts_for(&self, species: &[String]) -> Vec<SpeciesFact> {
        self.catalog
            .facts
            .iter()
            .filter(|fact| species.contains(&fact.species))
            .cloned()
            .collect()
    }

    pub(crate) fn sign_view(&self, sign: &super::Concession) -> SignView {
        let at = Position {
            x: sign.x,
            y: sign.y,
        };
        let subject = self.sign_subject(at);
        let reach = self.guest_reach();
        let readable = self
            .concession_customer_tiles(sign.id)
            .into_iter()
            .any(|tile| reach.contains(self, tile));
        let (status, detail) = match (&subject, readable) {
            (None, _) => (
                "decorative",
                format!(
                    "Decorative: no habitat with animals within {SIGN_REACH} tiles, so it teaches nothing"
                ),
            ),
            (Some(_), false) => (
                "unreachable",
                "Guests cannot reach the path beside it, so no one reads it".to_owned(),
            ),
            (Some((habitat_id, _)), true) => {
                let label = self
                    .habitats
                    .iter()
                    .find(|h| h.id == *habitat_id)
                    .map_or_else(String::new, |habitat| habitat.species_labels());
                (
                    "teaching",
                    format!("Teaches passing guests about the {label} of habitat #{habitat_id}"),
                )
            }
        };
        let (habitat_id, species) =
            subject.map_or((None, Vec::new()), |(id, species)| (Some(id), species));
        SignView {
            id: sign.id,
            x: sign.x,
            y: sign.y,
            habitat_id,
            facts: self.facts_for(&species),
            species,
            status,
            detail,
        }
    }

    fn talk_view(&self, talk: &TalkSchedule) -> TalkView {
        let habitat = self.habitats.iter().find(|h| h.id == talk.habitat_id);
        let species = habitat.map_or_else(String::new, |habitat| habitat.species_labels());
        let open = self.care_jobs.iter().find(|job| {
            job.habitat_id == talk.habitat_id
                && job.category == CareCategory::Talk
                && matches!(job.state, CareJobState::Due | CareJobState::InProgress)
        });
        let attendees = open
            .and_then(|job| {
                self.education
                    .attendance
                    .iter()
                    .find(|attendance| attendance.job_id == job.id)
            })
            .map_or(0, |attendance| attendance.guest_ids.len() as u32);
        let next = || format!("Next talk {}", clock(talk.next_due_minute));
        let (status, detail) = match open {
            Some(job) if job.state == CareJobState::Due => {
                let wait = self
                    .care_wait(job)
                    .map_or_else(|| "A keeper is being found".to_owned(), |wait| wait.message);
                (
                    "waiting_for_keeper",
                    format!(
                        "{wait}; canceled at {} without a keeper",
                        clock(job.due_minute + TALK_GRACE_MINUTES)
                    ),
                )
            }
            Some(job) => {
                let working = self.keepers.iter().any(|keeper| {
                    matches!(keeper.activity, KeeperActivity::Working { job_id, .. } if job_id == job.id)
                });
                if working {
                    (
                        "in_progress",
                        format!("{attendees} of {} places taken", talk.places),
                    )
                } else {
                    (
                        "keeper_on_the_way",
                        format!(
                            "The keeper is on the way; {attendees} of {} places reserved",
                            talk.places
                        ),
                    )
                }
            }
            None => match habitat {
                Some(habitat) if !habitat.stocked() => {
                    ("blocked", "No animals on show to talk about".to_owned())
                }
                Some(_) if self.guest_viewpoint(talk.habitat_id).is_none() => (
                    "blocked",
                    "Guests cannot reach a viewpoint beside the fence".to_owned(),
                ),
                _ => ("scheduled", next()),
            },
        };
        TalkView {
            habitat_id: talk.habitat_id,
            species,
            start_minute_of_day: talk.start_minute_of_day,
            duration_minutes: talk.duration_minutes,
            places: talk.places,
            next_due_minute: talk.next_due_minute,
            status,
            detail,
            attendees,
            last: self
                .education
                .recent_talks
                .iter()
                .rev()
                .find(|outcome| outcome.habitat_id == talk.habitat_id)
                .cloned(),
        }
    }

    fn achievement_progress(&self, kind: AchievementKind) -> String {
        let now = self.absolute_minute();
        match kind {
            AchievementKind::ThrivingExhibit => self
                .education
                .streaks
                .iter()
                .max_by_key(|streak| {
                    (
                        now - streak.since_minute,
                        std::cmp::Reverse(streak.habitat_id),
                    )
                })
                .map_or_else(
                    || {
                        format!(
                            "No habitat keeps healthy animals at welfare {THRIVING_WELFARE}+ yet"
                        )
                    },
                    |streak| {
                        format!(
                            "Habitat #{}: {} of {} hours at welfare {THRIVING_WELFARE}+",
                            streak.habitat_id,
                            (now - streak.since_minute) / 60,
                            THRIVING_MINUTES / 60
                        )
                    },
                ),
            AchievementKind::VetRecovery => {
                let ill = self
                    .habitats
                    .iter()
                    .flat_map(|habitat| habitat.animals.iter())
                    .find(|animal| animal.health.is_ill());
                match ill {
                    Some(animal) => format!(
                        "{} (#{}) is ill: a vet's treatment and rest lead to recovery",
                        animal.name, animal.id
                    ),
                    None => "No animal has needed a vet yet".to_owned(),
                }
            }
            AchievementKind::SupportedRehoming => {
                self.education.recent_rehomings.last().map_or_else(
                    || "No animal has moved to a partner zoo yet".to_owned(),
                    |rehoming| rehoming.detail.clone(),
                )
            }
        }
    }

    pub(crate) fn education_view(&self) -> EducationView {
        let learners: Vec<u32> = self
            .guests
            .iter()
            .map(|guest| learning(&guest.lessons))
            .filter(|points| *points > 0)
            .collect();
        let learners_now = learners.len() as u32;
        let average_learning = learners
            .iter()
            .sum::<u32>()
            .checked_div(learners_now)
            .unwrap_or(0);
        let signs: Vec<SignView> = self.signs().map(|sign| self.sign_view(sign)).collect();
        let talks: Vec<TalkView> = self
            .education
            .talks
            .iter()
            .map(|talk| self.talk_view(talk))
            .collect();
        let achievements: Vec<AchievementView> = AchievementKind::ALL
            .into_iter()
            .map(|kind| {
                let earned = self
                    .education
                    .achievements
                    .iter()
                    .find(|achievement| achievement.kind == kind)
                    .cloned();
                let progress = earned.as_ref().map_or_else(
                    || self.achievement_progress(kind),
                    |achievement| achievement.detail.clone(),
                );
                AchievementView {
                    kind,
                    title: kind.title(),
                    description: kind.description(),
                    earned,
                    progress,
                }
            })
            .collect();
        let grants = self
            .education
            .grants
            .iter()
            .map(|grant| GrantView {
                id: grant.id.clone(),
                title: grant.title.clone(),
                condition: grant.goal.describe(),
                amount_cents: grant.amount_cents,
                awarded_minute: grant.awarded_minute,
                progress: match grant.awarded_minute {
                    Some(minute) => format!("Paid {}", clock(minute)),
                    None => self.goal_progress(&grant.goal),
                },
            })
            .collect();
        let mut reasons = Vec::new();
        if !signs.iter().any(|sign| sign.status == "teaching") {
            reasons.push(format!(
                "Build an education sign beside a public path within {SIGN_REACH} tiles of a stocked habitat."
            ));
        }
        if talks.is_empty() {
            reasons.push(
                "Schedule a daily keeper talk at a stocked habitat guests can see.".to_owned(),
            );
        }
        for talk in &talks {
            if matches!(talk.status, "blocked" | "waiting_for_keeper") {
                reasons.push(format!(
                    "Habitat #{} talk: {}",
                    talk.habitat_id, talk.detail
                ));
            }
        }
        if learners_now > 0 && self.education.total.donors == 0 {
            reasons.push(format!(
                "Guests who learned something donate when they leave content (happiness {DONATION_MIN_HAPPINESS}+) with money to spare."
            ));
        }
        EducationView {
            signs,
            talks,
            learning: LearningView {
                learners_now,
                average_learning,
                today: self.education.today,
                total: self.education.total,
            },
            donations: self
                .education
                .donations
                .iter()
                .map(|entry| DonationSourceView {
                    habitat_id: entry.habitat_id,
                    label: self
                        .habitats
                        .iter()
                        .find(|h| h.id == entry.habitat_id)
                        .map_or_else(
                            || format!("Former habitat #{}", entry.habitat_id),
                            |habitat| {
                                format!("Habitat #{} · {}", habitat.id, habitat.species_labels())
                            },
                        ),
                    donors: entry.donors,
                    cents: entry.cents,
                })
                .collect(),
            recent_talks: self.education.recent_talks.clone(),
            achievements,
            recent_rehomings: self.education.recent_rehomings.clone(),
            grants,
            reasons,
        }
    }

    /// How far an education goal has come.
    fn goal_progress(&self, goal: &Goal) -> String {
        let (have, need, what) = match goal {
            Goal::GuestsEducated { at_least } => (
                self.education.total.guests_educated,
                *at_least,
                "guests taught",
            ),
            Goal::TalksGiven { at_least } => {
                (self.education.total.talks_given, *at_least, "talks given")
            }
            Goal::Achievements { at_least } => (
                self.education.achievements.len() as u32,
                *at_least,
                "milestones earned",
            ),
            _ => return "In progress".to_owned(),
        };
        format!("{} of {need} {what}", have.min(need))
    }

    /// Structural consistency of the saved education state.
    pub(crate) fn education_violation(&self) -> Option<String> {
        let education = &self.education;
        let mut previous = None;
        for talk in &education.talks {
            if previous.is_some_and(|id| id >= talk.habitat_id) {
                return Some("talk schedules are not one per habitat in order".to_owned());
            }
            previous = Some(talk.habitat_id);
            if !self.habitats.iter().any(|h| h.id == talk.habitat_id) {
                return Some(format!("talk for missing habitat {}", talk.habitat_id));
            }
            if talk.start_minute_of_day >= MINUTES_PER_DAY as u32
                || !(MIN_TALK_MINUTES..=MAX_TALK_MINUTES).contains(&talk.duration_minutes)
                || !(1..=MAX_TALK_PLACES).contains(&talk.places)
            {
                return Some(format!(
                    "talk at habitat {} is out of range",
                    talk.habitat_id
                ));
            }
        }
        let mut seen = HashSet::new();
        for attendance in &education.attendance {
            let open_talk = self.care_jobs.iter().any(|job| {
                job.id == attendance.job_id
                    && job.category == CareCategory::Talk
                    && job.habitat_id == attendance.habitat_id
            });
            if !open_talk {
                return Some(format!(
                    "talk places for missing talk {}",
                    attendance.job_id
                ));
            }
            if attendance.guest_ids.len() > attendance.places as usize {
                return Some(format!("talk {} is over-booked", attendance.job_id));
            }
            for guest_id in &attendance.guest_ids {
                if !seen.insert(*guest_id) {
                    return Some(format!("guest {guest_id} holds two talk places"));
                }
            }
        }
        let mut kinds = HashSet::new();
        if education
            .achievements
            .iter()
            .any(|achievement| !kinds.insert(achievement.kind))
        {
            return Some("a milestone is recorded twice".to_owned());
        }
        if education.recovered_animals.windows(2).any(|w| w[0] >= w[1]) {
            return Some("recovered animals are not unique and ascending".to_owned());
        }
        let mut grant_ids = HashSet::new();
        for grant in &education.grants {
            if !grant_ids.insert(grant.id.as_str()) || !grant.goal.education_goal() {
                return Some(format!("grant {} is duplicated or invalid", grant.id));
            }
        }
        for counts in [&education.today, &education.total] {
            if counts.donations_cents < 0 || counts.grants_cents < 0 {
                return Some("education money counts are negative".to_owned());
            }
        }
        for guest in &self.guests {
            let mut species = HashSet::new();
            let total: u32 = guest.lessons.iter().map(|lesson| lesson.points).sum();
            if total > MAX_LEARNING
                || guest
                    .lessons
                    .iter()
                    .any(|lesson| lesson.points == 0 || !species.insert(lesson.species.as_str()))
            {
                return Some(format!("guest {} has invalid lessons", guest.id));
            }
        }
        None
    }
}
