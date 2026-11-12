//! Structured park alerts (#94): the park's current problems, each with a stable reason code,
//! a severity, the object it concerns, where to look, and the action most likely to fix it.
//!
//! Alerts are read from authoritative park state after every command and every simulated
//! minute, never from the browser. One condition on one object is one alert however long it
//! lasts: it keeps its first occurrence and refreshes its last occurrence and message while the
//! condition holds, and it disappears as soon as the condition is resolved or its object is
//! gone. Alerts are saved with the park so their first occurrence survives a reload.
use super::{
    FenceSide, GameState, Habitat, Mover, Position, care::CareJobState, comfort::Need, shape,
};
use serde::{Deserialize, Serialize};

/// Welfare below this raises an alert; below the critical line it is critical.
pub(crate) const LOW_WELFARE: u32 = 40;
const CRITICAL_WELFARE: u32 = 25;
/// Feed stock at or below this many crates is low while animals are stocked.
const LOW_FEED_CRATES: u32 = 2;
/// Litter left this long is a backlog even with janitors hired.
const LITTER_BACKLOG_MINUTES: u64 = 120;
/// Due care waiting this long for a crew is late (half its grace hour).
const CARE_LATE_MINUTES: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AlertCode {
    FeedDepotEmpty,
    FeedDepotLow,
    GateUnreachable,
    HabitatUnstaffed,
    LowWelfare,
    UnsuitableGroup,
    DestinationIsolated,
    FacilityFailed,
    FacilityUnserviced,
    /// A dirty toilet or full bin no janitor is answering (#101).
    FacilityNeedsService,
    /// Guests who need a toilet or a seat and found none they could use (#101).
    ToiletDemandUnmet,
    RestDemandUnmet,
    /// An open stand with no vendor posted (#102).
    StandUnstaffed,
    /// An open stand sold out with no delivery on order (#102).
    StandOutOfStock,
    /// A stand whose queue is full: guests are being turned away (#102).
    StandQueueFull,
    ViewpointCrowded,
    LitterBacklog,
    KeeperNeedsZone,
    /// Scheduled care half an hour past due with no crew under way (#104).
    CareLate,
    /// Scheduled care that expired unserved within the last day (#104).
    CareMissed,
    /// Animals building risk of an illness from a care failure or a sick mate (#109).
    AnimalAtRisk,
    /// An unwell animal whose treatment waits for a vet, medicine or a route (#109).
    AnimalUnwell,
    /// A recovered quarantine patient that cannot return home (#109).
    QuarantineReleaseBlocked,
    /// A habitat getting less pool water or climate control than its animals need (#113).
    WaterShortage,
    ClimateShortage,
    /// A working utility plant whose capacity ran out before every habitat it reaches (#113).
    UtilityOverloaded,
    /// Cash covers less than a day of recurring commitments (#115).
    LowCash,
    /// Cash is below zero: purchases wait while costs keep accruing (#115).
    CashDeficit,
    /// A habitat without shade for every animal on a hot day, today or tomorrow (#118).
    HeatShade,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Severity {
    Critical,
    Warning,
    Notice,
}

/// The object an alert or overlay item concerns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub(crate) enum Subject {
    Depot,
    /// The path network as a whole (litter backlog).
    Paths,
    Habitat(u32),
    Concession(u32),
    Keeper(u32),
    /// One animal (#109).
    Animal(u32),
    /// The park's finances as a whole (#115).
    Finances,
}

/// The suggested fix; the browser maps it to a command, tool or inspector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AlertAction {
    BuyFeed,
    HireKeeper,
    HireJanitor,
    HireMechanic,
    BuildPath,
    DrawZone,
    AssignKeeper,
    AdoptAnimals,
    InspectHabitat,
    InspectFacility,
    BuildToilet,
    BuildBench,
    /// Veterinary fixes (#109).
    HireVet,
    BuyMedicine,
    InspectAnimal,
    /// Utility fixes (#113).
    BuildWaterPump,
    BuildClimatePlant,
    /// Open the finances report (#115).
    OpenFinances,
    /// Add a shelter to the habitat for shade (#118).
    AddShelter,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Alert {
    pub code: AlertCode,
    pub severity: Severity,
    pub subject: Subject,
    /// The tile to look at: the cause itself, such as the cut-off gate or crowded viewpoint.
    pub focus: Position,
    pub message: String,
    pub action: AlertAction,
    /// Park minutes (since day 1, 00:00) when the condition was first and last observed.
    pub first_minute: u64,
    pub last_minute: u64,
}

/// Tiles reachable over paths from a set of start tiles, indexed like `GameState::tiles`.
pub(crate) struct Reach(Vec<bool>);

impl Reach {
    pub(crate) fn contains(&self, state: &GameState, position: Position) -> bool {
        state
            .index(position.x, position.y)
            .is_some_and(|index| self.0[index])
    }
}

impl GameState {
    /// A flood over walkable tiles from every walkable start tile.
    pub(crate) fn reach_from(&self, mover: Mover, starts: &[Position]) -> Reach {
        let mut reached = vec![false; self.tiles.len()];
        let mut queue = std::collections::VecDeque::new();
        for start in starts {
            if self.walkable(mover, *start)
                && let Some(index) = self.index(start.x, start.y)
                && !reached[index]
            {
                reached[index] = true;
                queue.push_back(*start);
            }
        }
        while let Some(current) = queue.pop_front() {
            for next in self.neighbors(current) {
                if let Some(index) = self.index(next.x, next.y)
                    && !reached[index]
                    && self.walkable(mover, next)
                {
                    reached[index] = true;
                    queue.push_back(next);
                }
            }
        }
        Reach(reached)
    }

    pub(crate) fn guest_reach(&self) -> Reach {
        self.reach_from(Mover::Guest, &[self.entrance])
    }

    pub(crate) fn staff_reach(&self) -> Reach {
        self.reach_from(Mover::Staff, &self.depot_access_tiles())
    }

    /// The habitat's bounding-box centre, or a middle tile when that lies outside its fence.
    pub(crate) fn habitat_center(habitat: &Habitat) -> Position {
        let (x, y, width, height) = shape::bounds(&habitat.tiles);
        let center = Position {
            x: x + width / 2,
            y: y + height / 2,
        };
        if habitat.contains(center) {
            return center;
        }
        habitat.tiles[habitat.tiles.len() / 2]
    }

    /// The weakest welfare part of a habitat, by name and score.
    pub(crate) fn lowest_welfare_part(habitat: &Habitat) -> (&'static str, u32) {
        habitat
            .welfare_parts()
            .into_iter()
            .map(|part| {
                let name = match part.key {
                    "social" => "social group",
                    "ground_cover" => "ground cover",
                    "shallow_water" => "shallow water",
                    "deep_water" => "deep water",
                    key => key,
                };
                (name, part.score)
            })
            .min_by_key(|(_, score)| *score)
            .expect("welfare parts are not empty")
    }

    /// A stocked habitat whose group is too small for its species or too big for its fence.
    pub(crate) fn group_problem(&self, habitat: &Habitat) -> Option<(String, AlertAction)> {
        if habitat.residents.is_empty() {
            return None;
        }
        // Each species' group counts only its own kind (#108).
        let short = habitat.residents.iter().find_map(|resident| {
            let species = &resident.species;
            let minimum = species.welfare_profile().minimum_social_group.max(1);
            let count = habitat.species_count(&species.key);
            (count < minimum).then_some((species, count, minimum))
        });
        if let Some((species, count, minimum)) = short {
            Some((
                format!(
                    "Habitat #{} has {count} {}; they are social and need at least {minimum} \
                     together. Adopt more of the same species.",
                    habitat.id,
                    species.label()
                ),
                AlertAction::AdoptAnimals,
            ))
        } else if habitat.space_score() < 100 {
            Some((
                format!(
                    "Habitat #{} is crowded: {} {} need more room than its {} tiles. Build a \
                     larger habitat for this group.",
                    habitat.id,
                    habitat.animal_count(),
                    habitat.species_labels(),
                    habitat.area()
                ),
                AlertAction::InspectHabitat,
            ))
        } else {
            None
        }
    }

    /// Stands with an open maintenance request nobody can answer: no mechanic is hired, or
    /// none of the stand's service tiles connects to the depot.
    fn facility_unserviced(&self, concession_id: u32, staff: &Reach) -> Option<AlertAction> {
        let task = self
            .maintenance
            .iter()
            .find(|task| task.concession_id == concession_id)?;
        if task.assigned_mechanic_id.is_some() {
            return None;
        }
        if self.mechanics.is_empty() {
            return Some(AlertAction::HireMechanic);
        }
        let reachable = self
            .concession_service_tiles(concession_id)
            .into_iter()
            .any(|tile| staff.contains(self, tile));
        (!reachable).then_some(AlertAction::BuildPath)
    }

    /// Toilet and bench demand nobody meets, and dirty toilets or full bins no janitor answers
    /// (#101).
    fn detect_comfort_alerts(
        &self,
        staff: &Reach,
        raise: &mut impl FnMut(AlertCode, Severity, Subject, Position, String, AlertAction),
    ) {
        for (need, code, action, wanted, fix) in [
            (
                Need::Toilet,
                AlertCode::ToiletDemandUnmet,
                AlertAction::BuildToilet,
                "a toilet",
                "Build toilets beside a public path they can reach",
            ),
            (
                Need::Rest,
                AlertCode::RestDemandUnmet,
                AlertAction::BuildBench,
                "a seat",
                "Place benches beside the paths they walk",
            ),
        ] {
            let waiting: Vec<_> = self.unmet_guests(need).collect();
            let Some(first) = waiting.first() else {
                continue;
            };
            let count = waiting.len();
            raise(
                code,
                Severity::Warning,
                Subject::Paths,
                Position {
                    x: first.x,
                    y: first.y,
                },
                format!(
                    "{count} guest{} need{} {wanted} and found none open with room. {fix}.",
                    if count == 1 { "" } else { "s" },
                    if count == 1 { "s" } else { "" },
                ),
                action,
            );
        }
        for (index, stand) in self.concessions.iter().enumerate() {
            let Some(task) = self
                .facility_service
                .iter()
                .find(|task| task.concession_id == stand.id)
            else {
                continue;
            };
            if task.assigned_janitor_id.is_some() {
                continue;
            }
            let action = if self.janitors.is_empty() {
                AlertAction::HireJanitor
            } else if !self
                .concession_service_tiles(stand.id)
                .into_iter()
                .any(|tile| staff.contains(self, tile))
            {
                AlertAction::BuildPath
            } else {
                continue;
            };
            let closed =
                stand.kind == super::ConcessionKind::Toilet && !self.facility_accepting(index);
            let problem = match stand.kind {
                super::ConcessionKind::Bin => "is full, so new litter lands on the path",
                _ if closed => "is too dirty to use and closed",
                _ => "is getting dirty",
            };
            raise(
                AlertCode::FacilityNeedsService,
                if closed {
                    Severity::Critical
                } else {
                    Severity::Warning
                },
                Subject::Concession(stand.id),
                Position {
                    x: stand.x,
                    y: stand.y,
                },
                format!(
                    "{} #{} {problem}, and {}.",
                    stand.definition.label,
                    stand.id,
                    if action == AlertAction::HireJanitor {
                        "no janitor is hired to service it"
                    } else {
                        "no janitor can reach it from the depot"
                    }
                ),
                action,
            );
        }
    }

    fn detect_alerts(&self, now: u64) -> Vec<Alert> {
        let mut alerts = Vec::new();
        let mut raise = |code, severity, subject, focus, message: String, action| {
            alerts.push(Alert {
                code,
                severity,
                subject,
                focus,
                message,
                action,
                first_minute: now,
                last_minute: now,
            });
        };
        let guests = self.guest_reach();
        let staff = self.staff_reach();
        let occupancy = self.viewing_occupancy();
        let stocked = self.habitats.iter().any(|habitat| habitat.stocked());

        if stocked && self.feed_crates == 0 {
            raise(
                AlertCode::FeedDepotEmpty,
                Severity::Critical,
                Subject::Depot,
                self.depot,
                "The depot is out of feed: feeding jobs wait and animals go hungry. Buy feed \
                 crates at the depot."
                    .to_owned(),
                AlertAction::BuyFeed,
            );
        } else if stocked && self.feed_crates <= LOW_FEED_CRATES {
            raise(
                AlertCode::FeedDepotLow,
                Severity::Notice,
                Subject::Depot,
                self.depot,
                format!(
                    "Only {} feed crate{} left at the depot; buy more before feeding stops.",
                    self.feed_crates,
                    if self.feed_crates == 1 { "" } else { "s" }
                ),
                AlertAction::BuyFeed,
            );
        }

        for habitat in self.habitats.iter().filter(|habitat| habitat.stocked()) {
            let subject = Subject::Habitat(habitat.id);
            let center = Self::habitat_center(habitat);
            let gate_access = self.gate_access_tile(habitat.service_gate);
            if !self.service_gate_reachable(habitat) {
                raise(
                    AlertCode::GateUnreachable,
                    Severity::Critical,
                    subject,
                    gate_access.unwrap_or(center),
                    format!(
                        "Keepers cannot reach habitat #{}: its service gate has no path from the \
                         depot. Extend a path to the gate, or move the gate beside a path.",
                        habitat.id
                    ),
                    AlertAction::BuildPath,
                );
            }
            if let Some(problem) = self.staffing_problem(habitat) {
                let action = if self.habitat_zone(habitat).is_none() {
                    AlertAction::DrawZone
                } else if !self.has_keepers() {
                    AlertAction::HireKeeper
                } else {
                    AlertAction::AssignKeeper
                };
                raise(
                    AlertCode::HabitatUnstaffed,
                    Severity::Critical,
                    subject,
                    gate_access.unwrap_or(center),
                    format!(
                        "Habitat #{} has animals but no care: {problem}.",
                        habitat.id
                    ),
                    action,
                );
            }
            if let Some((message, action)) = self.care_late(habitat, now) {
                raise(
                    AlertCode::CareLate,
                    Severity::Warning,
                    subject,
                    gate_access.unwrap_or(center),
                    message,
                    action,
                );
            }
            let missed = self
                .care_jobs
                .iter()
                .filter(|job| {
                    job.habitat_id == habitat.id
                        && job.category.is_routine()
                        && job.state == CareJobState::Missed
                })
                .count();
            if missed > 0 {
                raise(
                    AlertCode::CareMissed,
                    Severity::Warning,
                    subject,
                    center,
                    format!(
                        "Habitat #{} missed {missed} scheduled care job{} in the last day; its \
                         animals went without. Add keepers to its zone or lower other priorities.",
                        habitat.id,
                        if missed == 1 { "" } else { "s" }
                    ),
                    AlertAction::InspectHabitat,
                );
            }
            if habitat.welfare() < LOW_WELFARE {
                let (part, score) = Self::lowest_welfare_part(habitat);
                raise(
                    AlertCode::LowWelfare,
                    if habitat.welfare() < CRITICAL_WELFARE {
                        Severity::Critical
                    } else {
                        Severity::Warning
                    },
                    subject,
                    center,
                    format!(
                        "Habitat #{} welfare is {}; its lowest part is {part} at {score}.",
                        habitat.id,
                        habitat.welfare()
                    ),
                    AlertAction::InspectHabitat,
                );
            }
            if let Some((message, action)) = self.group_problem(habitat) {
                raise(
                    AlertCode::UnsuitableGroup,
                    Severity::Warning,
                    subject,
                    center,
                    message,
                    action,
                );
            }
            let spots = self.viewing_spots_with_occupancy(habitat, &occupancy);
            if !spots
                .iter()
                .any(|spot| spot.capacity > 0 && guests.contains(self, spot.position))
            {
                raise(
                    AlertCode::DestinationIsolated,
                    Severity::Warning,
                    subject,
                    center,
                    format!(
                        "Guests cannot see habitat #{}: no path from the entrance runs beside a \
                         fence with animals in view. Connect a path along its fence.",
                        habitat.id
                    ),
                    AlertAction::BuildPath,
                );
            }
            if let Some(spot) = spots
                .iter()
                .filter(|spot| spot.crowded())
                .max_by_key(|spot| spot.occupancy.saturating_sub(spot.capacity))
            {
                let side = match spot.side {
                    FenceSide::North => "north",
                    FenceSide::East => "east",
                    FenceSide::South => "south",
                    FenceSide::West => "west",
                };
                raise(
                    AlertCode::ViewpointCrowded,
                    Severity::Warning,
                    subject,
                    spot.position,
                    format!(
                        "{} guests crowd the {side} viewpoint of habitat #{}, which holds {}. Run \
                         a path along another side of the fence so guests spread out.",
                        spot.occupancy, habitat.id, spot.capacity
                    ),
                    AlertAction::BuildPath,
                );
            }
        }

        for stand in &self.concessions {
            let subject = Subject::Concession(stand.id);
            let focus = Position {
                x: stand.x,
                y: stand.y,
            };
            let label = &stand.definition.label;
            let unserviced = self.facility_unserviced(stand.id, &staff);
            let broken = if stand.kind.sells() {
                "has broken down and makes no sales"
            } else if stand.kind.is_utility() {
                "has failed and supplies nothing"
            } else {
                "has broken down and is closed"
            };
            if self.concession_service_state(stand) == "failed" {
                raise(
                    AlertCode::FacilityFailed,
                    Severity::Critical,
                    subject,
                    focus,
                    format!(
                        "{label} #{} {broken} ({}% condition).{}",
                        stand.id,
                        stand.condition,
                        match unserviced {
                            Some(AlertAction::HireMechanic) => " Hire a mechanic to repair it.",
                            Some(_) => " No mechanic can reach it: connect it to the depot.",
                            None => " A mechanic is on the way.",
                        }
                    ),
                    unserviced.unwrap_or(AlertAction::InspectFacility),
                );
            } else if let Some(action) = unserviced {
                raise(
                    AlertCode::FacilityUnserviced,
                    Severity::Warning,
                    subject,
                    focus,
                    format!(
                        "{label} #{} needs maintenance ({}% condition) but {}.",
                        stand.id,
                        stand.condition,
                        if action == AlertAction::HireMechanic {
                            "no mechanic is hired"
                        } else {
                            "no mechanic can reach it from the depot"
                        }
                    ),
                    action,
                );
            }
            if !stand.kind.is_utility()
                && !self
                    .concession_customer_tiles(stand.id)
                    .into_iter()
                    .any(|tile| guests.contains(self, tile))
            {
                raise(
                    AlertCode::DestinationIsolated,
                    Severity::Warning,
                    subject,
                    focus,
                    format!(
                        "Guests cannot reach {label} #{}: connect it to the entrance path.",
                        stand.id
                    ),
                    AlertAction::BuildPath,
                );
            }
            // Staffed stands (#102): the problems the player fixes at the stand itself.
            if let (Some(ops), Some(service)) = (&stand.stand, &stand.definition.stand)
                && ops.open
                && self.concession_service_state(stand) != "failed"
            {
                if ops.vendors == 0 {
                    raise(
                        AlertCode::StandUnstaffed,
                        Severity::Warning,
                        subject,
                        focus,
                        format!(
                            "{label} #{} is open but has no vendor, so it sells nothing. Post a \
                             vendor at the stand.",
                            stand.id
                        ),
                        AlertAction::InspectFacility,
                    );
                } else if ops.stock == 0 && ops.delivery_due.is_none() {
                    raise(
                        AlertCode::StandOutOfStock,
                        Severity::Warning,
                        subject,
                        focus,
                        format!(
                            "{label} #{} has sold out and no delivery is on order. Restock it \
                             or turn automatic restocking on.",
                            stand.id
                        ),
                        AlertAction::InspectFacility,
                    );
                } else if ops.queue.len() as u32 >= service.queue_capacity {
                    raise(
                        AlertCode::StandQueueFull,
                        Severity::Notice,
                        subject,
                        focus,
                        format!(
                            "{label} #{}'s queue is full and guests are turned away. Post \
                             another vendor or build a second stand.",
                            stand.id
                        ),
                        AlertAction::InspectFacility,
                    );
                }
            }
        }

        self.detect_comfort_alerts(&staff, &mut raise);

        if let Some(oldest) = self.litter.iter().min_by_key(|task| task.created_minute) {
            let unreachable = self.litter.iter().find(|task| {
                !staff.contains(
                    self,
                    Position {
                        x: task.x,
                        y: task.y,
                    },
                )
            });
            let age = now.saturating_sub(oldest.created_minute);
            let (focus, reason, action) = if self.janitors.is_empty() {
                (
                    oldest,
                    "no janitor is hired".to_owned(),
                    AlertAction::HireJanitor,
                )
            } else if let Some(task) = unreachable {
                (
                    task,
                    format!(
                        "the litter at {},{} has no path from the depot",
                        task.x, task.y
                    ),
                    AlertAction::BuildPath,
                )
            } else if age >= LITTER_BACKLOG_MINUTES {
                (
                    oldest,
                    format!("the oldest has waited {age} minutes"),
                    AlertAction::HireJanitor,
                )
            } else {
                (oldest, String::new(), AlertAction::HireJanitor)
            };
            if !reason.is_empty() {
                raise(
                    AlertCode::LitterBacklog,
                    Severity::Warning,
                    Subject::Paths,
                    Position {
                        x: focus.x,
                        y: focus.y,
                    },
                    format!(
                        "{} litter on the paths is not being cleaned: {reason}. Dirty paths \
                         spoil guest visits.",
                        self.litter.len()
                    ),
                    action,
                );
            }
        }

        self.detect_health_alerts(&mut raise);
        self.detect_utility_alerts(&mut raise);
        self.detect_finance_alerts(&mut raise);
        self.detect_heat_alerts(&mut raise);

        for keeper in self.keepers.iter().filter(|keeper| {
            keeper.zone_id.is_none() && keeper.role == super::care::CareRole::Keeper
        }) {
            raise(
                AlertCode::KeeperNeedsZone,
                Severity::Notice,
                Subject::Keeper(keeper.id),
                Position {
                    x: keeper.x,
                    y: keeper.y,
                },
                format!(
                    "Keeper #{} is idle without a work zone; assign them to one.",
                    keeper.id
                ),
                if self.work_zones.is_empty() {
                    AlertAction::DrawZone
                } else {
                    AlertAction::AssignKeeper
                },
            );
        }
        alerts
    }

    /// Health warnings before illness, unwell animals whose treatment waits, and recovered
    /// patients who cannot go home (#109). Each says why and what fixes it.
    fn detect_health_alerts(
        &self,
        raise: &mut impl FnMut(AlertCode, Severity, Subject, Position, String, AlertAction),
    ) {
        for habitat in &self.habitats {
            let at_risk: Vec<_> = habitat
                .animals
                .iter()
                .filter_map(|animal| Some((animal, animal.health.warning()?)))
                .collect();
            let Some(&(first, condition)) = at_risk.first() else {
                continue;
            };
            let why = condition.cause(habitat).unwrap_or_else(|| {
                self.contagion_source(habitat, first.id).map_or_else(
                    || "its risk is easing".to_owned(),
                    |(name, id)| format!("{name} (#{id}) has the sniffles nearby"),
                )
            });
            let who = if at_risk.len() == 1 {
                format!("{} (#{})", first.name, first.id)
            } else {
                format!(
                    "{} (#{}) and {} other animal{}",
                    first.name,
                    first.id,
                    at_risk.len() - 1,
                    if at_risk.len() == 2 { "" } else { "s" }
                )
            };
            let fix = if condition.cause(habitat).is_some() {
                condition.fix()
            } else {
                "Have a vet treat the patient or move it into quarantine"
            };
            raise(
                AlertCode::AnimalAtRisk,
                Severity::Notice,
                Subject::Habitat(habitat.id),
                first.position,
                format!(
                    "{who} in habitat #{} may come down with {}: {why}. {fix}.",
                    habitat.id,
                    condition.label()
                ),
                AlertAction::InspectHabitat,
            );
        }
        for (animal_id, habitat, reason, action) in self.waiting_patients() {
            let Some(animal) = habitat.animals.iter().find(|animal| animal.id == animal_id) else {
                continue;
            };
            let label = animal
                .health
                .illness
                .map_or("an illness", |illness| illness.condition.label());
            raise(
                AlertCode::AnimalUnwell,
                Severity::Warning,
                Subject::Animal(animal_id),
                animal.position,
                format!(
                    "{} (#{animal_id}) in habitat #{} is unwell with {label} and waits for a vet: {reason}.",
                    animal.name, habitat.id
                ),
                action,
            );
        }
        for habitat in self.habitats.iter().filter(|habitat| habitat.quarantine) {
            for animal in habitat.animals.iter().filter(|a| !a.health.is_ill()) {
                if let Some(reason) = self.release_blocker(animal.id) {
                    raise(
                        AlertCode::QuarantineReleaseBlocked,
                        Severity::Notice,
                        Subject::Animal(animal.id),
                        animal.position,
                        format!(
                            "{} (#{}) has recovered in quarantine but cannot go home: {reason}.",
                            animal.name, animal.id
                        ),
                        AlertAction::InspectAnimal,
                    );
                }
            }
        }
    }

    /// Re-reads every alert condition, keeping each still-present alert's first occurrence.
    /// Utility allocation (#113) is refreshed first, so a command that builds, removes,
    /// repairs or disconnects a plant shows its effect at once, even while paused.
    pub(crate) fn update_alerts(&mut self) {
        self.refresh_utilities();
        let now = self.absolute_minute();
        let previous = std::mem::take(&mut self.alerts);
        let mut alerts = self.detect_alerts(now);
        for alert in &mut alerts {
            if let Some(old) = previous
                .iter()
                .find(|old| old.code == alert.code && old.subject == alert.subject)
            {
                alert.first_minute = old.first_minute;
            }
        }
        alerts.sort_by(|left, right| {
            (left.severity, left.first_minute, left.code, left.subject).cmp(&(
                right.severity,
                right.first_minute,
                right.code,
                right.subject,
            ))
        });
        self.alerts = alerts;
    }

    /// The habitat's oldest due job that has waited [`CARE_LATE_MINUTES`] for a crew, said
    /// with the specific reason it waits and the fix for it (#104).
    fn care_late(&self, habitat: &Habitat, now: u64) -> Option<(String, AlertAction)> {
        let job = self
            .care_jobs
            .iter()
            .filter(|job| {
                job.habitat_id == habitat.id
                    && job.category.is_routine()
                    && job.state == CareJobState::Due
                    && now >= job.due_minute + CARE_LATE_MINUTES
            })
            .min_by_key(|job| (job.due_minute, job.id))?;
        let wait = self.care_wait(job)?;
        Some((
            format!(
                "Habitat #{} {} is {} min late: {}.",
                habitat.id,
                job.category.key(),
                now - job.due_minute,
                wait.message
            ),
            wait.action,
        ))
    }

    /// Open care work the zone's keepers have not finished: due or in progress.
    pub(crate) fn open_care_jobs(&self, habitat_id: u32) -> usize {
        self.care_jobs
            .iter()
            .filter(|job| {
                job.habitat_id == habitat_id
                    && job.category.is_routine()
                    && matches!(job.state, CareJobState::Due | CareJobState::InProgress)
            })
            .count()
    }
}
