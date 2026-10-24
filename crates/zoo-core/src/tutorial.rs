//! The guided introduction (#93, building on #137's keeper training): a guided scenario's
//! objectives become a compact checklist where the next objective shows its authored hint, a
//! pointer to the tool or object it needs, and the park's own current blocker for it.
//!
//! Progress is the scenario's objective progress, read from authoritative park state after
//! every command and advance, never from UI clicks or prescribed coordinates, so any valid
//! layout and any step order count. The guide only observes: it never issues commands,
//! creates jobs or grants money, so skipping, restarting, loading or replaying the command log
//! cannot duplicate jobs or rewards. Its state lives beside the simulation rules, not inside
//! them, and is saved with the park.
use super::{
    ActionResult, FenceSide, GameState, Habitat, Position,
    construction::dollars,
    scenario::{Goal, GuideTool},
    session::DomainEvent,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TutorialStatus {
    /// The park's scenario is not guided; its objectives are plain goals.
    Off,
    Active,
    Skipped,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tutorial {
    status: TutorialStatus,
    /// The latest refused command while guiding, so the guide can explain how to recover.
    /// Session-only: a reloaded park starts without a pending refusal.
    #[serde(skip)]
    refusal: Option<String>,
}

impl Tutorial {
    pub(crate) fn active() -> Self {
        Self {
            status: TutorialStatus::Active,
            refusal: None,
        }
    }

    pub(crate) fn off() -> Self {
        Self {
            status: TutorialStatus::Off,
            refusal: None,
        }
    }

    /// Remembers why the latest command was refused; a successful command clears it.
    pub(crate) fn note_result(&mut self, result: &ActionResult) {
        if self.status == TutorialStatus::Active {
            self.refusal = (!result.ok).then(|| result.message.clone());
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TutorialView {
    status: TutorialStatus,
    /// The habitat being taught: the first stocked habitat, else the first habitat.
    habitat_id: Option<u32>,
    /// The first unfinished objective while guiding.
    next: Option<String>,
    hint: Option<String>,
    tool: Option<GuideTool>,
    /// The park's current blocker for the next objective, or how to recover from a refusal.
    problem: Option<String>,
}

impl GameState {
    fn tutorial_habitat(&self) -> Option<&Habitat> {
        self.habitats
            .iter()
            .find(|habitat| habitat.stocked())
            .or_else(|| self.habitats.first())
    }

    /// Completes the guide once every objective is done; called after objectives update.
    pub(crate) fn update_tutorial(&mut self) {
        if self.tutorial.status != TutorialStatus::Active
            || !self.objectives.iter().all(|objective| objective.done)
        {
            return;
        }
        self.tutorial.status = TutorialStatus::Completed;
        self.tutorial.refusal = None;
        self.pending_events.push(DomainEvent {
            tick: self.elapsed_minutes,
            kind: "tutorial_completed",
            entity_id: None,
        });
    }

    pub(crate) fn skip_tutorial(&mut self) -> ActionResult {
        if self.tutorial.status != TutorialStatus::Active {
            return ActionResult::error("The park guide is not running");
        }
        self.tutorial.status = TutorialStatus::Skipped;
        self.tutorial.refusal = None;
        ActionResult::ok("Guide skipped; restart it from the park manager")
    }

    /// Restarts the guide: every objective is re-checked against the park as it is now, so
    /// steps the park already satisfies tick again at once. The park itself is untouched.
    pub(crate) fn restart_tutorial(&mut self) -> ActionResult {
        if self.tutorial.status == TutorialStatus::Off {
            return ActionResult::error("This scenario has no guide");
        }
        self.tutorial = Tutorial::active();
        for objective in &mut self.objectives {
            objective.done = false;
        }
        ActionResult::ok("Guide restarted")
    }

    pub(crate) fn tutorial_view(&self) -> TutorialView {
        // A latched step whose structure was undone or demolished is the next step again, so
        // the guide points at rebuilding it rather than at a step that now cannot happen.
        let next = (self.tutorial.status == TutorialStatus::Active)
            .then(|| {
                self.objectives.iter().find(|objective| {
                    !objective.done
                        || (objective.goal.structural() && !self.goal_reached(&objective.goal))
                })
            })
            .flatten();
        TutorialView {
            status: self.tutorial.status,
            habitat_id: self.tutorial_habitat().map(|habitat| habitat.id),
            next: next.map(|objective| objective.id.clone()),
            hint: next.and_then(|objective| objective.hint.clone()),
            tool: next.and_then(|objective| objective.tool),
            problem: next.and_then(|objective| self.guide_problem(&objective.goal)),
        }
    }

    /// The one most useful thing standing between the park and `goal`, read from park state.
    fn guide_problem(&self, goal: &Goal) -> Option<String> {
        // Other refusals already explain themselves where the command was made; the park's
        // own blocker below is the more useful pointer for them.
        if let Some(refusal) = &self.tutorial.refusal
            && refusal.starts_with("Not enough cash")
        {
            return Some(format!(
                "{refusal}. You have {}. Undo construction you no longer need today for a full \
                 refund, or let guest admissions and stand sales build up first.",
                dollars(self.cash_cents)
            ));
        }
        let habitat = self.tutorial_habitat();
        let specific = match goal {
            Goal::GatesReachable => self
                .habitats
                .iter()
                .find(|habitat| !self.service_gate_reachable(habitat))
                .map(|habitat| self.unreachable_gate_problem(habitat)),
            Goal::ZonedHabitats { .. } | Goal::StaffedHabitats { .. } => {
                habitat.and_then(|habitat| self.guide_staffing_problem(habitat))
            }
            Goal::SuitableGroups { .. } => self.unsuitable_group_problem(),
            Goal::Guests { .. } => self.guest_problem(),
            Goal::CareJobsCompleted { .. } => habitat.and_then(|habitat| {
                self.care_job_views(habitat.id)
                    .into_iter()
                    .find_map(|job| job.waiting_reason)
            }),
            Goal::Welfare { at_least } => self.welfare_problem(*at_least),
            _ => None,
        };
        specific.or_else(|| self.standing_problem())
    }

    fn unreachable_gate_problem(&self, habitat: &Habitat) -> String {
        let side = match habitat.service_gate.side {
            FenceSide::North => "north",
            FenceSide::East => "east",
            FenceSide::South => "south",
            FenceSide::West => "west",
        };
        match self.gate_access_tile(habitat.service_gate) {
            Some(Position { x, y }) => format!(
                "Habitat #{}'s service gate, on its {side} fence (tile {x},{y}), has no path \
                 from the depot. Extend the path to it, or move the gate to a side along a \
                 connected path.",
                habitat.id
            ),
            None => format!(
                "Habitat #{}'s service gate faces the park edge; move it to a side along a path.",
                habitat.id
            ),
        }
    }

    fn guide_staffing_problem(&self, habitat: &Habitat) -> Option<String> {
        let problem = self.zone_staffing_problem(habitat)?;
        if self.habitat_zone(habitat).is_some() && self.keepers.is_empty() {
            return Some(format!(
                "{problem}. You have no keepers yet: hire one at the depot first."
            ));
        }
        Some(problem)
    }

    fn unsuitable_group_problem(&self) -> Option<String> {
        self.habitats
            .iter()
            .find_map(|habitat| self.group_problem(habitat).map(|(message, _)| message))
    }

    fn guest_problem(&self) -> Option<String> {
        if !self.habitats.iter().any(|habitat| habitat.stocked()) {
            return Some("Guests come to see animals: stock a habitat first.".to_owned());
        }
        let start = self.entrance;
        let occupancy = self.viewing_occupancy();
        let viewable = self.habitats.iter().any(|habitat| {
            habitat.stocked()
                && self
                    .viewing_route_with_occupancy(habitat, start, &occupancy)
                    .is_some()
        });
        (!viewable).then(|| {
            "Guests need a path from the entrance that runs beside a stocked habitat's fence; \
             each viewing spot along it holds only a few guests."
                .to_owned()
        })
    }

    /// The lowest welfare component of the worst stocked habitat below `at_least`.
    fn welfare_problem(&self, at_least: u32) -> Option<String> {
        let habitat = self
            .habitats
            .iter()
            .filter(|habitat| habitat.stocked() && habitat.welfare < at_least)
            .min_by_key(|habitat| habitat.welfare)?;
        let (name, score) = Self::lowest_welfare_part(habitat);
        let trend = if habitat.welfare_target() > habitat.welfare {
            "rising"
        } else {
            "not rising"
        };
        Some(format!(
            "Habitat #{} welfare is {} and {trend}; its lowest part is {name} at {score}. Welfare \
             blends social group and space with food, water, cleanliness and the habitat layout: \
             shelter, enrichment, ground cover and pools.",
            habitat.id, habitat.welfare
        ))
    }

    /// Problems that matter whatever the next step is: stocked animals nobody can care for, or
    /// an empty feed store.
    fn standing_problem(&self) -> Option<String> {
        let stocked = || self.habitats.iter().filter(|habitat| habitat.stocked());
        if let Some(problem) = stocked().find_map(|habitat| {
            self.staffing_problem(habitat).map(|problem| {
                format!("Habitat #{} has animals but no care: {problem}", habitat.id)
            })
        }) {
            return Some(problem);
        }
        (stocked().next().is_some() && self.feed_crates == 0).then(|| {
            "The depot is out of feed: feeding jobs wait and animals go hungry. Buy feed crates \
             at the depot."
                .to_owned()
        })
    }
}
