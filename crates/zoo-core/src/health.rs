//! Animal health, veterinary treatment and quarantine (#109).
//!
//! Health is a small authored model, not a disease simulator, and it stays recoverable and
//! gentle by default (Decision 6.1.12): an illness never kills, never spreads park-wide and
//! never leaves lasting harm. Every rule is deterministic and runs on the park clock.
//!
//! **Risk and warning.** Each animal builds risk towards each [`Condition`] while the care
//! failure that causes it persists in its habitat (low food for an upset tummy, a dirty
//! habitat for the sniffles), [`HEALTH_INTERVAL_MINUTES`] at a time, and loses it at the same
//! rate once the failure is fixed. Halfway to onset the animal shows a warning naming the
//! cause; at [`ONSET_RISK_MINUTES`] it becomes unwell with that condition.
//!
//! **Transmission.** The sniffles is the one contagious condition, and it spreads only inside
//! one habitat: an untreated unwell animal builds sniffles risk in its healthy habitat mates
//! as a dirty habitat would. Treatment or quarantine stops it. Nothing spreads inside a
//! quarantine habitat, whose patients are kept apart.
//!
//! **Treatment.** An unwell animal rests where it is and generates one treatment job. Vets,
//! not keepers, take it (the 2026-10-06 owner decision): the scheduler of #104 dispatches the
//! nearest free vet, who collects a medicine kit from the depot, walks the paths to the
//! habitat's service gate and on to the patient, and treats it there. Only then is the kit
//! used and the animal recovering. No vet, no medicine or no route each leave the job blocked
//! with its reason; there is no remote treatment.
//!
//! **Recovery.** A treated animal recovers over [`RECOVERY_MINUTES`] of good care. While the
//! cause of its condition persists where it lives, recovery pauses (it never regresses).
//!
//! **Quarantine.** The player may set an empty habitat aside for quarantine. An ill animal can
//! move in when a quarantine habitat has room, its gate is reachable and the usual arrival
//! rules pass; it remembers its home habitat, lives only in the quarantine habitat while it
//! is there, and returns home by itself once recovered and its home can take it back.
use super::{
    ActionResult, ExpenseCategory, GameState, Habitat, Keeper,
    alerts::AlertAction,
    care::{self, CareJob, CareJobState, CareRole, KeeperActivity},
    content::{self, CareCategory},
};
use serde::{Deserialize, Serialize};

/// How often health is assessed.
pub(crate) const HEALTH_INTERVAL_MINUTES: u32 = 15;
/// Exposure at which an animal shows a warning: half of the onset.
pub(crate) const WARNING_RISK_MINUTES: u32 = 4 * 60;
/// Exposure at which an animal falls ill.
pub(crate) const ONSET_RISK_MINUTES: u32 = 8 * 60;
/// Good care a treated animal needs to recover.
pub(crate) const RECOVERY_MINUTES: u32 = 6 * 60;
/// How long a vet works with a patient.
pub(crate) const TREATMENT_WORK_MINUTES: u32 = 20;
/// Medicine kits in one depot purchase.
pub(crate) const MEDICINE_BATCH_KITS: u32 = 5;
/// Below this feed level an animal builds upset-tummy risk.
const HUNGRY_FOOD: u32 = 20;
/// Below this cleanliness an animal builds sniffles risk.
const DIRTY_CLEANLINESS: u32 = 30;

/// The authored conditions, in assessment order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Condition {
    UpsetTummy,
    Sniffles,
}

impl Condition {
    pub const ALL: [Self; 2] = [Self::UpsetTummy, Self::Sniffles];

    pub fn label(self) -> &'static str {
        match self {
            Self::UpsetTummy => "an upset tummy",
            Self::Sniffles => "the sniffles",
        }
    }

    /// What the player sees on the animal: gentle and non-graphic.
    pub fn symptom(self) -> &'static str {
        match self {
            Self::UpsetTummy => "off its food and resting",
            Self::Sniffles => "sneezing and resting",
        }
    }

    pub fn contagious(self) -> bool {
        matches!(self, Self::Sniffles)
    }

    /// The care failure in `habitat` that builds this condition's risk, if it is present.
    pub fn cause(self, habitat: &Habitat) -> Option<String> {
        match self {
            Self::UpsetTummy if habitat.food < HUNGRY_FOOD => {
                Some(format!("habitat #{} is low on food", habitat.id))
            }
            Self::Sniffles if habitat.cleanliness < DIRTY_CLEANLINESS => {
                Some(format!("habitat #{} is dirty", habitat.id))
            }
            _ => None,
        }
    }

    /// The fix for [`Condition::cause`].
    pub fn fix(self) -> &'static str {
        match self {
            Self::UpsetTummy => "Check that its keepers can feed it",
            Self::Sniffles => "Check that its keepers can clean it",
        }
    }
}

/// Minutes of exposure built towards each condition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Risk {
    pub upset_tummy: u32,
    pub sniffles: u32,
}

impl Risk {
    pub fn get(self, condition: Condition) -> u32 {
        match condition {
            Condition::UpsetTummy => self.upset_tummy,
            Condition::Sniffles => self.sniffles,
        }
    }

    fn get_mut(&mut self, condition: Condition) -> &mut u32 {
        match condition {
            Condition::UpsetTummy => &mut self.upset_tummy,
            Condition::Sniffles => &mut self.sniffles,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Stage {
    /// Waiting for a vet; the animal rests where it is.
    Unwell,
    /// Treated; recovering while it is well cared for.
    Recovering { remaining_minutes: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Illness {
    pub condition: Condition,
    pub onset_minute: u64,
    pub stage: Stage,
}

/// One animal's health record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Health {
    pub risk: Risk,
    pub illness: Option<Illness>,
    /// While the animal is in quarantine: the habitat it returns to once recovered.
    pub quarantine_home: Option<u32>,
}

impl Health {
    pub fn is_unwell(&self) -> bool {
        matches!(
            self.illness,
            Some(Illness {
                stage: Stage::Unwell,
                ..
            })
        )
    }

    pub fn is_ill(&self) -> bool {
        self.illness.is_some()
    }

    /// The condition closest to onset once its risk reaches the warning level.
    pub fn warning(&self) -> Option<Condition> {
        if self.is_ill() {
            return None;
        }
        Condition::ALL
            .into_iter()
            .filter(|&condition| self.risk.get(condition) >= WARNING_RISK_MINUTES)
            .max_by_key(|&condition| (self.risk.get(condition), std::cmp::Reverse(condition)))
    }

    /// How well the animal counts towards its group's health: unwell 0, recovering 50.
    pub fn score(&self) -> u32 {
        match self.illness {
            None => 100,
            Some(Illness {
                stage: Stage::Unwell,
                ..
            }) => 0,
            Some(Illness {
                stage: Stage::Recovering { .. },
                ..
            }) => 50,
        }
    }
}

/// Where an animal's treatment stands, for the player.
#[derive(Serialize)]
pub(crate) struct TreatmentView {
    pub job_id: u32,
    pub phase: care::CarePhase,
    pub vet_id: Option<u32>,
    pub waiting_reason: Option<String>,
    /// The fix for a waiting treatment: hire a vet, buy medicine or build a path.
    pub action: Option<AlertAction>,
}

/// An animal's health as the player sees it.
#[derive(Serialize)]
pub(crate) struct HealthView {
    /// `healthy`, `at_risk`, `unwell` or `recovering`.
    pub status: &'static str,
    pub condition: Option<Condition>,
    /// One sentence: what is wrong, why, and what happens next.
    pub summary: String,
    /// The highest risk as a share of onset.
    pub risk_percent: u32,
    pub recovery_percent: Option<u32>,
    /// Why recovery is paused, if it is.
    pub recovery_paused: Option<String>,
    pub treatment: Option<TreatmentView>,
    pub in_quarantine: bool,
    pub quarantine_home: Option<u32>,
    /// Why a recovered patient cannot go home yet.
    pub release_blocked: Option<String>,
}

/// Whether an animal can move into quarantine, and where or why not.
#[derive(Serialize)]
pub(crate) struct QuarantinePreview {
    pub ok: bool,
    pub message: String,
    pub blockers: Vec<String>,
    pub habitat_id: Option<u32>,
}

impl QuarantinePreview {
    fn refused(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            ok: false,
            blockers: vec![message.clone()],
            message,
            habitat_id: None,
        }
    }
}

impl GameState {
    pub(crate) fn vet_count(&self) -> usize {
        self.keepers
            .iter()
            .filter(|keeper| keeper.role == CareRole::Vet)
            .count()
    }

    /// Whether any keeper (not vet) is hired.
    pub(crate) fn has_keepers(&self) -> bool {
        self.keepers.len() > self.vet_count()
    }

    /// Risk, onset, transmission and recovery for every animal, then homecomings.
    pub(crate) fn advance_health(&mut self) {
        let now = self.absolute_minute();
        let step = HEALTH_INTERVAL_MINUTES;
        let mut events = Vec::new();
        for habitat in &mut self.habitats {
            let causes = Condition::ALL.map(|condition| condition.cause(habitat).is_some());
            let cause = |condition: Condition| causes[condition as usize];
            // The habitat's contagious patient, if any: nothing spreads inside quarantine.
            let source = (!habitat.quarantine)
                .then(|| {
                    habitat
                        .animals
                        .iter()
                        .filter(|animal| contagious(&animal.health))
                        .map(|animal| animal.id)
                        .min()
                })
                .flatten();
            for animal in &mut habitat.animals {
                let health = &mut animal.health;
                match health.illness.as_mut() {
                    None => {
                        for condition in Condition::ALL {
                            let exposed = cause(condition)
                                || (condition.contagious()
                                    && source.is_some_and(|source| source != animal.id));
                            let risk = health.risk.get_mut(condition);
                            *risk = if exposed {
                                risk.saturating_add(step).min(ONSET_RISK_MINUTES)
                            } else {
                                risk.saturating_sub(step)
                            };
                        }
                        if let Some(condition) = Condition::ALL
                            .into_iter()
                            .find(|&condition| health.risk.get(condition) >= ONSET_RISK_MINUTES)
                        {
                            health.risk = Risk::default();
                            health.illness = Some(Illness {
                                condition,
                                onset_minute: now,
                                stage: Stage::Unwell,
                            });
                            events.push(("animal_fell_ill", animal.id));
                        }
                    }
                    Some(Illness {
                        stage: Stage::Unwell,
                        ..
                    }) => {}
                    Some(Illness {
                        condition,
                        stage: Stage::Recovering { remaining_minutes },
                        ..
                    }) => {
                        if !cause(*condition) {
                            *remaining_minutes = remaining_minutes.saturating_sub(step);
                            if *remaining_minutes == 0 {
                                health.illness = None;
                                events.push(("animal_recovered", animal.id));
                            }
                        }
                    }
                }
            }
        }
        for (kind, id) in events {
            self.emit(kind, Some(id));
            if kind == "animal_recovered" {
                self.note_recovery(id);
            }
        }
        self.release_recovered();
    }

    /// A vet's completed treatment: the patient starts recovering. Only an unwell patient
    /// is treated, so one illness is treated at most once.
    pub(crate) fn begin_recovery(&mut self, animal_id: u32) {
        let Some((index, at)) = self.locate_animal(animal_id) else {
            return;
        };
        let health = &mut self.habitats[index].animals[at].health;
        if !health.is_unwell() {
            return;
        }
        if let Some(illness) = health.illness.as_mut() {
            illness.stage = Stage::Recovering {
                remaining_minutes: RECOVERY_MINUTES,
            };
        }
        self.emit("animal_treated", Some(animal_id));
    }

    /// One treatment job for every unwell animal without an open one.
    pub(crate) fn generate_treatment_jobs(&mut self, now: u64) {
        let unwell: Vec<(u32, u32)> = self
            .habitats
            .iter()
            .flat_map(|habitat| {
                habitat
                    .animals
                    .iter()
                    .filter(|animal| animal.health.is_unwell())
                    .map(move |animal| (habitat.id, animal.id))
            })
            .collect();
        for (habitat_id, animal_id) in unwell {
            if self.open_treatment(animal_id).is_some() {
                continue;
            }
            let id = self.next_care_job_id;
            self.next_care_job_id = self.next_care_job_id.saturating_add(1);
            self.care_jobs.push(CareJob {
                id,
                habitat_id,
                category: CareCategory::Treatment,
                due_minute: now,
                priority: content::MAX_CARE_PRIORITY,
                required_keepers: 1,
                work_minutes: TREATMENT_WORK_MINUTES,
                state: CareJobState::Due,
                keeper_ids: Vec::new(),
                supply: care::JobSupply::None,
                resolved_minute: None,
                animal_id: Some(animal_id),
            });
        }
    }

    /// The patient's open treatment job, if any.
    pub(crate) fn open_treatment(&self, animal_id: u32) -> Option<&CareJob> {
        self.care_jobs.iter().find(|job| {
            job.animal_id == Some(animal_id)
                && matches!(job.state, CareJobState::Due | CareJobState::InProgress)
        })
    }

    /// Keeps treatments with their patients: a patient that moved (into quarantine, or by
    /// transfer) is treated where it now lives, so a vet already under way hands the job back
    /// and it follows the patient; a patient that left the park or is no longer unwell needs
    /// no treatment, so its job is withdrawn and any kit goes back to the depot.
    pub(crate) fn reconcile_treatment_jobs(&mut self, now: u64) {
        let mut withdrawn = Vec::new();
        let mut moved = Vec::new();
        for job in &self.care_jobs {
            let Some(animal_id) = job.animal_id else {
                continue;
            };
            if !matches!(job.state, CareJobState::Due | CareJobState::InProgress) {
                continue;
            }
            match self.locate_animal(animal_id) {
                Some((index, at)) if self.habitats[index].animals[at].health.is_unwell() => {
                    let habitat_id = self.habitats[index].id;
                    if habitat_id != job.habitat_id {
                        moved.push((job.id, habitat_id));
                    }
                }
                _ => withdrawn.push(job.id),
            }
        }
        for (job_id, habitat_id) in moved {
            self.interrupt_care_job(job_id, now);
            if let Some(job) = self.care_jobs.iter_mut().find(|job| job.id == job_id) {
                job.habitat_id = habitat_id;
            }
        }
        for &job_id in &withdrawn {
            self.interrupt_care_job(job_id, now);
        }
        if !withdrawn.is_empty() {
            self.care_jobs.retain(|job| !withdrawn.contains(&job.id));
            for job_id in withdrawn {
                self.emit("treatment_withdrawn", Some(job_id));
            }
        }
    }

    /// Hires a vet at the depot. Vets work the whole park and take only treatments.
    pub(crate) fn hire_vet(&mut self) -> ActionResult {
        if let Err(message) = self.spend(
            self.catalog.balance.staff.vet.hire_cents,
            ExpenseCategory::VetHiring,
        ) {
            return ActionResult::error(message);
        }
        let id = self.next_keeper_id;
        self.next_keeper_id = self.next_keeper_id.saturating_add(1);
        self.keepers.push(Keeper {
            id,
            role: CareRole::Vet,
            zone_id: None,
            schedule: care::KeeperSchedule::vet(),
            deliveries_completed: 0,
            x: self.depot.x,
            y: self.depot.y,
            activity: KeeperActivity::AtDepot,
            exit: None,
            carrying_feed: false,
            carrying_medicine: false,
        });
        ActionResult::ok(format!("Vet #{id} hired at the animal-care depot"))
    }

    pub(crate) fn buy_medicine(&mut self) -> ActionResult {
        if let Err(message) = self.spend(
            self.catalog.balance.medicine_batch_cents,
            ExpenseCategory::Medicine,
        ) {
            return ActionResult::error(message);
        }
        self.medicine_kits = self.medicine_kits.saturating_add(MEDICINE_BATCH_KITS);
        ActionResult::ok(format!(
            "Bought {MEDICINE_BATCH_KITS} medicine kits for the animal-care depot"
        ))
    }

    /// Sets an empty habitat aside for quarantine, or returns a quarantine habitat without
    /// patients to ordinary use.
    pub(crate) fn set_habitat_quarantine(
        &mut self,
        habitat_id: u32,
        enabled: bool,
    ) -> ActionResult {
        let Some(index) = self.find_habitat(habitat_id) else {
            return ActionResult::error("Select a habitat first");
        };
        let habitat = &self.habitats[index];
        if habitat.quarantine == enabled {
            return ActionResult::ok(if enabled {
                format!("Habitat #{habitat_id} is already set aside for quarantine")
            } else {
                format!("Habitat #{habitat_id} is already an ordinary habitat")
            });
        }
        if enabled && habitat.stocked() {
            return ActionResult::error(format!(
                "Move habitat #{habitat_id}'s animals out before setting it aside for quarantine"
            ));
        }
        if !enabled && habitat.stocked() {
            let patients = habitat.animal_count();
            return ActionResult::error(format!(
                "Habitat #{habitat_id} still cares for {patients} patient{}; they return home once recovered",
                if patients == 1 { "" } else { "s" }
            ));
        }
        self.habitats[index].quarantine = enabled;
        self.emit("habitat_quarantine_changed", Some(habitat_id));
        ActionResult::ok(if enabled {
            format!("Habitat #{habitat_id} is set aside for quarantine")
        } else {
            format!("Habitat #{habitat_id} is an ordinary habitat again")
        })
    }

    /// Previews moving an ill animal into the first quarantine habitat that can take it.
    pub(crate) fn evaluate_quarantine(&self, animal_id: u32) -> QuarantinePreview {
        let Some((from, at)) = self.locate_animal(animal_id) else {
            return QuarantinePreview::refused(format!("Animal #{animal_id} does not exist"));
        };
        let source = &self.habitats[from];
        let animal = &source.animals[at];
        let name = &animal.name;
        if !animal.health.is_ill() {
            return QuarantinePreview::refused(format!(
                "{name} is healthy; quarantine is for unwell animals"
            ));
        }
        if source.quarantine {
            return QuarantinePreview::refused(format!(
                "{name} is already in quarantine in habitat #{}",
                source.id
            ));
        }
        let Some(species) = source
            .resident_index(&animal.species)
            .map(|resident| source.residents[resident].species.clone())
        else {
            return QuarantinePreview::refused("That animal has no resident species");
        };
        let mut blockers = Vec::new();
        let mut quarantines = self
            .habitats
            .iter()
            .enumerate()
            .filter(|(_, habitat)| habitat.quarantine)
            .peekable();
        if quarantines.peek().is_none() {
            return QuarantinePreview::refused(
                "No habitat is set aside for quarantine; set an empty habitat aside first",
            );
        }
        for (index, habitat) in quarantines {
            let mut reasons = Vec::new();
            if habitat.animal_count() >= habitat.capacity() {
                reasons.push(format!(
                    "Quarantine habitat #{} is full ({} of {} places)",
                    habitat.id,
                    habitat.animal_count(),
                    habitat.capacity()
                ));
            } else {
                if !self.service_gate_reachable(habitat) {
                    reasons.push(format!(
                        "Quarantine habitat #{} cannot be reached: its service gate has no path from the depot",
                        habitat.id
                    ));
                }
                reasons.extend(self.arrival_blockers(index, &species, 1).0);
            }
            if reasons.is_empty() {
                return QuarantinePreview {
                    ok: true,
                    message: format!(
                        "Move {name} (#{animal_id}) into quarantine habitat #{}",
                        habitat.id
                    ),
                    blockers: Vec::new(),
                    habitat_id: Some(habitat.id),
                };
            }
            blockers.extend(reasons);
        }
        QuarantinePreview {
            ok: false,
            message: blockers[0].clone(),
            blockers,
            habitat_id: None,
        }
    }

    /// Moves an ill animal into quarantine; it remembers its home habitat.
    pub(crate) fn quarantine_animal(&mut self, animal_id: u32) -> ActionResult {
        let preview = self.evaluate_quarantine(animal_id);
        let (true, Some(habitat_id)) = (preview.ok, preview.habitat_id) else {
            return ActionResult::error(preview.message);
        };
        let (from, _) = self
            .locate_animal(animal_id)
            .expect("the preview found the animal");
        let home = self.habitats[from].id;
        let name = self.relocate_animal(animal_id, habitat_id);
        if let Some((index, at)) = self.locate_animal(animal_id) {
            self.habitats[index].animals[at].health.quarantine_home = Some(home);
        }
        self.emit("animal_quarantined", Some(animal_id));
        ActionResult::ok(format!(
            "{name} (#{animal_id}) moved from habitat #{home} into quarantine habitat #{habitat_id}"
        ))
    }

    /// Why a recovered patient cannot go home yet, if it cannot.
    pub(crate) fn release_blocker(&self, animal_id: u32) -> Option<String> {
        let (from, at) = self.locate_animal(animal_id)?;
        let source = &self.habitats[from];
        let animal = &source.animals[at];
        let home_id = animal.health.quarantine_home?;
        let Some(home) = self.find_habitat(home_id) else {
            return Some(format!(
                "its home habitat #{home_id} is gone; move it to another habitat"
            ));
        };
        if self.habitats[home].quarantine {
            return Some(format!(
                "its home habitat #{home_id} is set aside for quarantine; move it to another habitat"
            ));
        }
        let species = source
            .resident_index(&animal.species)
            .map(|resident| source.residents[resident].species.clone())?;
        self.arrival_blockers(home, &species, 1)
            .0
            .into_iter()
            .next()
            .map(|reason| format!("{reason}; move it to another habitat"))
    }

    /// Recovered patients return home as soon as their home can take them back.
    fn release_recovered(&mut self) {
        let ready: Vec<(u32, u32)> = self
            .habitats
            .iter()
            .flat_map(|habitat| habitat.animals.iter())
            .filter(|animal| !animal.health.is_ill())
            .filter_map(|animal| Some((animal.id, animal.health.quarantine_home?)))
            .collect();
        for (animal_id, home) in ready {
            if self.release_blocker(animal_id).is_some() {
                continue;
            }
            self.relocate_animal(animal_id, home);
            if let Some((index, at)) = self.locate_animal(animal_id) {
                self.habitats[index].animals[at].health.quarantine_home = None;
            }
            self.emit("animal_released", Some(animal_id));
        }
    }

    /// The player's view of an animal's health.
    pub(crate) fn health_view(
        &self,
        habitat: &Habitat,
        animal: &super::animal::Animal,
    ) -> HealthView {
        let health = &animal.health;
        let risk_percent = Condition::ALL
            .into_iter()
            .map(|condition| health.risk.get(condition))
            .max()
            .unwrap_or(0)
            .saturating_mul(100)
            / ONSET_RISK_MINUTES;
        let in_quarantine = health.quarantine_home.is_some();
        let treatment = self.open_treatment(animal.id).map(|job| {
            let (phase, waiting_reason) = self.treatment_phase(job);
            TreatmentView {
                job_id: job.id,
                phase,
                vet_id: job.keeper_ids.first().copied(),
                waiting_reason,
                action: self.care_wait(job).map(|wait| wait.action),
            }
        });
        let release_blocked = (in_quarantine && !health.is_ill())
            .then(|| self.release_blocker(animal.id))
            .flatten();
        let mut view = HealthView {
            status: "healthy",
            condition: None,
            summary: "Healthy".to_owned(),
            risk_percent,
            recovery_percent: None,
            recovery_paused: None,
            treatment,
            in_quarantine,
            quarantine_home: health.quarantine_home,
            release_blocked,
        };
        match health.illness {
            None => {
                if let Some(condition) = health.warning() {
                    view.status = "at_risk";
                    view.condition = Some(condition);
                    let why = condition.cause(habitat).unwrap_or_else(|| {
                        self.contagion_source(habitat, animal.id).map_or_else(
                            || "its risk is easing".to_owned(),
                            |(name, id)| format!("{name} (#{id}) has the sniffles nearby"),
                        )
                    });
                    view.summary = format!(
                        "At risk of {}: {why}. {}.",
                        condition.label(),
                        if condition.cause(habitat).is_some() {
                            condition.fix()
                        } else {
                            "Have a vet treat the patient or move it into quarantine"
                        }
                    );
                } else if let Some(reason) = &view.release_blocked {
                    view.summary = format!("Recovered, but cannot go home: {reason}");
                }
            }
            Some(illness) => {
                view.condition = Some(illness.condition);
                match illness.stage {
                    Stage::Unwell => {
                        view.status = "unwell";
                        let next = match &view.treatment {
                            Some(TreatmentView {
                                phase: care::CarePhase::Servicing,
                                vet_id: Some(vet),
                                ..
                            }) => format!("Vet #{vet} is treating it"),
                            Some(TreatmentView {
                                vet_id: Some(vet), ..
                            }) => format!("Vet #{vet} is on the way"),
                            Some(TreatmentView {
                                waiting_reason: Some(reason),
                                ..
                            }) => format!("Waiting for a vet: {reason}"),
                            _ => "Waiting for a vet".to_owned(),
                        };
                        view.summary = format!(
                            "Unwell with {}: {}. {next}.",
                            illness.condition.label(),
                            illness.condition.symptom()
                        );
                    }
                    Stage::Recovering { remaining_minutes } => {
                        view.status = "recovering";
                        let done = RECOVERY_MINUTES.saturating_sub(remaining_minutes);
                        view.recovery_percent = Some(done * 100 / RECOVERY_MINUTES);
                        view.recovery_paused = illness.condition.cause(habitat);
                        view.summary = match &view.recovery_paused {
                            Some(cause) => format!(
                                "Treated for {}; recovery paused: {cause}. {}.",
                                illness.condition.label(),
                                illness.condition.fix()
                            ),
                            None => format!(
                                "Treated for {}; recovering · {}%",
                                illness.condition.label(),
                                done * 100 / RECOVERY_MINUTES
                            ),
                        };
                    }
                }
            }
        }
        view
    }

    /// The untreated contagious patient sharing `habitat` with animal `except`, if any.
    pub(crate) fn contagion_source(&self, habitat: &Habitat, except: u32) -> Option<(String, u32)> {
        if habitat.quarantine {
            return None;
        }
        habitat
            .animals
            .iter()
            .filter(|animal| animal.id != except && contagious(&animal.health))
            .min_by_key(|animal| animal.id)
            .map(|animal| (animal.name.clone(), animal.id))
    }

    /// The treatment's phase and the reason it waits, through the shared job semantics.
    fn treatment_phase(&self, job: &CareJob) -> (care::CarePhase, Option<String>) {
        if job.state == CareJobState::Due {
            return match self.care_wait(job) {
                Some(wait) if wait.blocked => (care::CarePhase::Blocked, Some(wait.message)),
                wait => (care::CarePhase::Queued, wait.map(|wait| wait.message)),
            };
        }
        let worker = job
            .keeper_ids
            .first()
            .and_then(|id| self.keepers.iter().find(|k| k.id == *id));
        let phase = match worker.map(|worker| worker.activity) {
            Some(KeeperActivity::CollectingStock { .. }) => care::CarePhase::CollectingStock,
            Some(KeeperActivity::Working { .. }) => care::CarePhase::Servicing,
            _ => care::CarePhase::Traveling,
        };
        (phase, None)
    }

    /// Unwell animals whose treatment waits, with the reason and fix (#109 alerts).
    pub(crate) fn waiting_patients(&self) -> Vec<(u32, &Habitat, String, AlertAction)> {
        let mut waiting = Vec::new();
        for job in &self.care_jobs {
            let (Some(animal_id), CareJobState::Due) = (job.animal_id, job.state) else {
                continue;
            };
            let Some(habitat) = self.habitats.iter().find(|h| h.id == job.habitat_id) else {
                continue;
            };
            if let Some(wait) = self.care_wait(job) {
                waiting.push((animal_id, habitat, wait.message, wait.action));
            }
        }
        waiting
    }

    /// Health invariants: one condition stage at a time, bounded risk and recovery, patients
    /// only in quarantine habitats with a recorded home, and at most one open treatment per
    /// animal, always for an unwell patient where it lives.
    pub(crate) fn health_violation(&self) -> Option<String> {
        for habitat in &self.habitats {
            for animal in &habitat.animals {
                let health = &animal.health;
                if Condition::ALL
                    .into_iter()
                    .any(|condition| health.risk.get(condition) > ONSET_RISK_MINUTES)
                {
                    return Some(format!("animal {} risk is above onset", animal.id));
                }
                if let Some(Illness {
                    stage: Stage::Recovering { remaining_minutes },
                    ..
                }) = health.illness
                    && (remaining_minutes == 0 || remaining_minutes > RECOVERY_MINUTES)
                {
                    return Some(format!("animal {} recovery is out of range", animal.id));
                }
                if habitat.quarantine != health.quarantine_home.is_some() {
                    return Some(format!(
                        "animal {} quarantine membership does not match habitat {}",
                        animal.id, habitat.id
                    ));
                }
                if health.quarantine_home == Some(habitat.id) {
                    return Some(format!("animal {} is quarantined at home", animal.id));
                }
            }
        }
        let mut patients = std::collections::HashSet::new();
        for job in &self.care_jobs {
            let treatment = job.category == CareCategory::Treatment;
            if treatment != job.animal_id.is_some() {
                return Some(format!("care job {} has a mismatched patient", job.id));
            }
            let (Some(animal_id), CareJobState::Due | CareJobState::InProgress) =
                (job.animal_id, job.state)
            else {
                continue;
            };
            if !patients.insert(animal_id) {
                return Some(format!("animal {animal_id} has two open treatments"));
            }
            let Some((index, at)) = self.locate_animal(animal_id) else {
                return Some(format!("care job {} treats a missing animal", job.id));
            };
            let habitat = &self.habitats[index];
            if habitat.id != job.habitat_id || !habitat.animals[at].health.is_unwell() {
                return Some(format!("care job {} treats no unwell patient", job.id));
            }
        }
        for keeper in &self.keepers {
            let vet = keeper.role == CareRole::Vet;
            if vet && keeper.zone_id.is_some() {
                return Some(format!("vet {} has a work zone", keeper.id));
            }
            if vet != keeper.schedule.accepts(CareCategory::Treatment) {
                return Some(format!(
                    "worker {} has the wrong care categories",
                    keeper.id
                ));
            }
            if keeper.carrying_feed && keeper.carrying_medicine {
                return Some(format!("worker {} carries two loads", keeper.id));
            }
        }
        None
    }
}

/// An untreated sniffles patient: the one way the condition spreads.
fn contagious(health: &Health) -> bool {
    matches!(
        health.illness,
        Some(Illness {
            condition,
            stage: Stage::Unwell,
            ..
        }) if condition.contagious()
    )
}
