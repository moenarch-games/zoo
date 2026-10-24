//! Keeper-management onboarding (#137): a compact checklist that teaches the
//! habitat-creates-work / keeper-accepts-work model through the player's first habitat.
//!
//! Progress is read from authoritative park state after every command and advance, never
//! from UI clicks or prescribed coordinates, so any valid layout and any step order count.
//! A step latches once reached. The tutorial only observes: it never issues commands,
//! creates jobs or grants money, so skipping, restarting or replaying the command log cannot
//! duplicate jobs or rewards. Its state lives beside the simulation rules, not inside them.
use super::{GameState, Habitat, care::CareJobState, session::DomainEvent};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TutorialStatus {
    Active,
    Skipped,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TutorialStep {
    BuildHabitat,
    ZoneGate,
    AssignKeeper,
    StockHabitat,
    JobGenerated,
    EditSchedule,
    JobCompleted,
}

impl TutorialStep {
    const ALL: [TutorialStep; 7] = [
        TutorialStep::BuildHabitat,
        TutorialStep::ZoneGate,
        TutorialStep::AssignKeeper,
        TutorialStep::StockHabitat,
        TutorialStep::JobGenerated,
        TutorialStep::EditSchedule,
        TutorialStep::JobCompleted,
    ];

    fn title(self) -> &'static str {
        match self {
            TutorialStep::BuildHabitat => "Build a habitat",
            TutorialStep::ZoneGate => "Put its service gate in a work zone",
            TutorialStep::AssignKeeper => "Assign a keeper to that zone",
            TutorialStep::StockHabitat => "Adopt animals",
            TutorialStep::JobGenerated => "See the habitat create care work",
            TutorialStep::EditSchedule => "Change one care task's priority",
            TutorialStep::JobCompleted => "Watch a keeper finish a care job",
        }
    }

    /// The idea this step teaches, shown while it is the next step.
    fn lesson(self) -> &'static str {
        match self {
            TutorialStep::BuildHabitat => {
                "Draw a closed fence anywhere on clear grass. Habitats are where care work comes from."
            }
            TutorialStep::ZoneGate => {
                "A habitat belongs to the work zone that contains its service gate, not its fence. \
                 Choose Zones and drag a rectangle around the gate; move the gate to change owner."
            }
            TutorialStep::AssignKeeper => {
                "Zones limit who may work: only keepers assigned to the zone take its habitats' jobs. \
                 Hire a keeper at the depot, then assign them in the zone inspector."
            }
            TutorialStep::StockHabitat => {
                "Adopting animals copies the species' care routine into the habitat: feeding, \
                 cleaning and inspection times and priorities are already set."
            }
            TutorialStep::JobGenerated => {
                "Select the habitat: Care lists each task's time, interval and when it is next due. \
                 Each due time becomes one job."
            }
            TutorialStep::EditSchedule => {
                "Priorities decide which job goes first when keepers are short. Raise or lower one \
                 task's priority in the habitat's Care list."
            }
            TutorialStep::JobCompleted => {
                "Keepers declare the care categories and shift hours they accept; a due job waits \
                 until an eligible keeper in the zone walks in through the gate."
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Tutorial {
    status: TutorialStatus,
    reached: Vec<TutorialStep>,
    /// The habitat being taught; re-chosen if it is demolished.
    habitat_id: Option<u32>,
}

impl Default for Tutorial {
    fn default() -> Self {
        Self {
            status: TutorialStatus::Active,
            reached: Vec::new(),
            habitat_id: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TutorialStepView {
    key: TutorialStep,
    title: &'static str,
    done: bool,
}

#[derive(Serialize)]
pub(crate) struct TutorialView {
    status: TutorialStatus,
    habitat_id: Option<u32>,
    steps: Vec<TutorialStepView>,
    /// The first unreached step, its lesson and the park's current blocker for it, if any.
    next: Option<TutorialStep>,
    lesson: Option<&'static str>,
    problem: Option<String>,
}

impl GameState {
    fn tutorial_habitat(&self) -> Option<&Habitat> {
        let chosen = self
            .tutorial
            .habitat_id
            .and_then(|id| self.habitats.iter().find(|habitat| habitat.id == id));
        chosen
            .or_else(|| self.habitats.iter().find(|habitat| habitat.animals > 0))
            .or_else(|| self.habitats.first())
    }

    fn tutorial_step_reached(&self, step: TutorialStep, habitat: &Habitat) -> bool {
        let jobs = || {
            self.care_jobs
                .iter()
                .filter(|job| job.habitat_id == habitat.id)
        };
        match step {
            TutorialStep::BuildHabitat => true,
            TutorialStep::ZoneGate => self.habitat_zone(habitat).is_some(),
            TutorialStep::AssignKeeper => self
                .habitat_zone(habitat)
                .is_some_and(|zone_id| self.zone_keeper_count(zone_id) > 0),
            TutorialStep::StockHabitat => habitat.animals > 0,
            TutorialStep::JobGenerated => jobs().next().is_some(),
            TutorialStep::EditSchedule => habitat
                .care_schedule
                .as_ref()
                .is_some_and(|schedule| schedule.tasks.iter().any(|task| task.customized)),
            TutorialStep::JobCompleted => jobs().any(|job| job.state == CareJobState::Completed),
        }
    }

    /// Latches every step the park now satisfies; called after each command and advance.
    pub(crate) fn update_tutorial(&mut self) {
        if self.tutorial.status != TutorialStatus::Active {
            return;
        }
        let Some(habitat) = self.tutorial_habitat() else {
            return;
        };
        let habitat_id = habitat.id;
        let newly: Vec<TutorialStep> = TutorialStep::ALL
            .into_iter()
            .filter(|step| !self.tutorial.reached.contains(step))
            .filter(|step| self.tutorial_step_reached(*step, habitat))
            .collect();
        self.tutorial.habitat_id = Some(habitat_id);
        for step in newly {
            self.tutorial.reached.push(step);
            self.pending_events.push(DomainEvent {
                tick: self.elapsed_minutes,
                kind: "tutorial_step_reached",
                entity_id: Some(habitat_id),
            });
        }
        if self.tutorial.reached.len() == TutorialStep::ALL.len() {
            self.tutorial.status = TutorialStatus::Completed;
            self.pending_events.push(DomainEvent {
                tick: self.elapsed_minutes,
                kind: "tutorial_completed",
                entity_id: Some(habitat_id),
            });
        }
    }

    pub(crate) fn skip_tutorial(&mut self) -> super::ActionResult {
        if self.tutorial.status != TutorialStatus::Active {
            return super::ActionResult::error("Keeper training is not running");
        }
        self.tutorial.status = TutorialStatus::Skipped;
        super::ActionResult::ok("Keeper training skipped; restart it from the park manager")
    }

    /// Restarts the checklist; steps the park already satisfies are ticked again at once.
    pub(crate) fn restart_tutorial(&mut self) -> super::ActionResult {
        self.tutorial = Tutorial::default();
        super::ActionResult::ok("Keeper training restarted")
    }

    pub(crate) fn tutorial_view(&self) -> TutorialView {
        let habitat = self.tutorial_habitat();
        let reached = |step: TutorialStep| self.tutorial.reached.contains(&step);
        let next = (self.tutorial.status == TutorialStatus::Active)
            .then(|| TutorialStep::ALL.into_iter().find(|step| !reached(*step)))
            .flatten();
        let problem = match (next, habitat) {
            (Some(TutorialStep::ZoneGate | TutorialStep::AssignKeeper), Some(habitat)) => {
                self.zone_staffing_problem(habitat)
            }
            (Some(TutorialStep::JobCompleted), Some(habitat)) => self
                .care_job_views(habitat.id)
                .into_iter()
                .find_map(|job| job.waiting_reason),
            _ => None,
        };
        TutorialView {
            status: self.tutorial.status,
            habitat_id: habitat.map(|habitat| habitat.id),
            steps: TutorialStep::ALL
                .into_iter()
                .map(|step| TutorialStepView {
                    key: step,
                    title: step.title(),
                    done: reached(step),
                })
                .collect(),
            next,
            lesson: next.map(TutorialStep::lesson),
            problem,
        }
    }
}
