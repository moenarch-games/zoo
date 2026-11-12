//! Management overlays and the daily summary (#94).
//!
//! An overlay grades park tiles for one concern (access, welfare, staff coverage, cleanliness,
//! facilities, guest viewing) straight from authoritative state, and lists the problem objects
//! it found so the browser can jump to them. It is computed on request, never per frame, and
//! never changes the park. The daily summary rolls up today's headline numbers together with
//! the entities and ledger lines each number is made of, so a drill-down always reconciles.
use super::{
    GameState, Habitat, PathAccess, Position, TileKind, alerts::Subject, care::CareJobState,
    comfort::Need, construction::dollars,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OverlayKind {
    Access,
    Welfare,
    Staff,
    Cleanliness,
    Facilities,
    Viewing,
    /// Utility coverage, plant capacity and habitat supply (#113).
    Utilities,
}

impl OverlayKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "access" => Self::Access,
            "welfare" => Self::Welfare,
            "staff" => Self::Staff,
            "cleanliness" => Self::Cleanliness,
            "facilities" => Self::Facilities,
            "viewing" => Self::Viewing,
            "utilities" => Self::Utilities,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Level {
    Good,
    Warn,
    Bad,
}

#[derive(Serialize)]
struct OverlayCell {
    x: u32,
    y: u32,
    level: Level,
    /// The graded number where one exists (welfare, condition, open jobs, guests, minutes).
    value: Option<u32>,
}

#[derive(Serialize)]
struct OverlayItem {
    level: Level,
    subject: Subject,
    focus: Position,
    label: String,
}

#[derive(Serialize)]
struct LegendEntry {
    level: Level,
    label: &'static str,
}

#[derive(Serialize)]
pub(crate) struct OverlayView {
    kind: OverlayKind,
    title: &'static str,
    legend: Vec<LegendEntry>,
    cells: Vec<OverlayCell>,
    /// Problem objects (warn or bad), worst first.
    items: Vec<OverlayItem>,
    good: u32,
    warn: u32,
    bad: u32,
}

#[derive(Default)]
struct Builder {
    cells: BTreeMap<(u32, u32), (Level, Option<u32>)>,
    items: Vec<OverlayItem>,
}

impl Builder {
    /// Grades a tile; a tile graded twice keeps its worse grade.
    fn cell(&mut self, position: Position, level: Level, value: Option<u32>) {
        let entry = self
            .cells
            .entry((position.y, position.x))
            .or_insert((level, value));
        if level > entry.0 {
            *entry = (level, value);
        }
    }

    fn footprint(&mut self, habitat: &Habitat, level: Level, value: Option<u32>) {
        for tile in &habitat.tiles {
            self.cell(*tile, level, value);
        }
    }

    fn item(&mut self, level: Level, subject: Subject, focus: Position, label: String) {
        if level != Level::Good {
            self.items.push(OverlayItem {
                level,
                subject,
                focus,
                label,
            });
        }
    }
}

fn grade(value: u32, good_from: u32, warn_from: u32) -> Level {
    if value >= good_from {
        Level::Good
    } else if value >= warn_from {
        Level::Warn
    } else {
        Level::Bad
    }
}

fn legend(entries: [&'static str; 3]) -> Vec<LegendEntry> {
    [Level::Good, Level::Warn, Level::Bad]
        .into_iter()
        .zip(entries)
        .map(|(level, label)| LegendEntry { level, label })
        .collect()
}

impl GameState {
    pub(crate) fn overlay(&self, kind: OverlayKind) -> OverlayView {
        let mut out = Builder::default();
        let (title, legend) = match kind {
            OverlayKind::Access => {
                self.access_overlay(&mut out);
                (
                    "Access",
                    legend([
                        "Connected (public to the entrance, staff-only to the depot)",
                        "Service gate cut off from the depot",
                        "Not connected",
                    ]),
                )
            }
            OverlayKind::Welfare => {
                for habitat in self.habitats.iter().filter(|habitat| habitat.stocked()) {
                    let level = grade(habitat.welfare(), 70, super::alerts::LOW_WELFARE);
                    out.footprint(habitat, level, Some(habitat.welfare()));
                    let (part, score) = Self::lowest_welfare_part(habitat);
                    out.item(
                        level,
                        Subject::Habitat(habitat.id),
                        Self::habitat_center(habitat),
                        format!(
                            "Habitat #{} welfare {} · lowest {part} {score}",
                            habitat.id,
                            habitat.welfare()
                        ),
                    );
                }
                (
                    "Animal welfare",
                    legend(["Welfare 70+", "Welfare 40–69", "Welfare below 40"]),
                )
            }
            OverlayKind::Staff => {
                for habitat in self.habitats.iter().filter(|habitat| habitat.stocked()) {
                    let open = self.open_care_jobs(habitat.id) as u32;
                    let keepers = self
                        .habitat_zone(habitat)
                        .map_or(0, |zone| self.zone_keeper_count(zone))
                        as u32;
                    let (level, label) = match self.staffing_problem(habitat) {
                        Some(problem) => (Level::Bad, problem),
                        None if open > keepers => (
                            Level::Warn,
                            format!("{open} open care jobs for {keepers} zone keeper(s)"),
                        ),
                        None => (Level::Good, String::new()),
                    };
                    out.footprint(habitat, level, Some(open));
                    out.item(
                        level,
                        Subject::Habitat(habitat.id),
                        Self::habitat_center(habitat),
                        format!("Habitat #{}: {label}", habitat.id),
                    );
                }
                for keeper in self.keepers.iter().filter(|keeper| {
                    keeper.zone_id.is_none() && keeper.role == crate::care::CareRole::Keeper
                }) {
                    let position = Position {
                        x: keeper.x,
                        y: keeper.y,
                    };
                    out.cell(position, Level::Warn, None);
                    out.item(
                        Level::Warn,
                        Subject::Keeper(keeper.id),
                        position,
                        format!("Keeper #{} has no work zone", keeper.id),
                    );
                }
                (
                    "Staff coverage",
                    legend([
                        "Covered by zone keepers",
                        "More open jobs than keepers",
                        "No keeper can care for it",
                    ]),
                )
            }
            OverlayKind::Cleanliness => {
                self.cleanliness_overlay(&mut out);
                (
                    "Cleanliness",
                    legend([
                        "Clean",
                        "Litter waiting / habitat getting dirty",
                        "Litter nobody cleans / dirty habitat",
                    ]),
                )
            }
            OverlayKind::Facilities => {
                self.facilities_overlay(&mut out);
                (
                    "Facilities",
                    legend([
                        "Working",
                        "Worn or low on stock",
                        "Broken down or out of stock",
                    ]),
                )
            }
            OverlayKind::Viewing => {
                self.viewing_overlay(&mut out);
                (
                    "Guest viewing",
                    legend([
                        "Room to watch",
                        "Viewpoint full",
                        "Viewpoint over capacity or crowded path",
                    ]),
                )
            }
            OverlayKind::Utilities => {
                self.utilities_overlay(&mut out);
                (
                    "Utilities",
                    legend([
                        "Supplied, or path a working plant covers",
                        "Short (warning hour) or plant worn or overloaded",
                        "Degraded habitat or failed plant",
                    ]),
                )
            }
        };
        out.items.sort_by_key(|item| std::cmp::Reverse(item.level));
        let count = |level| {
            out.cells
                .values()
                .filter(|(cell, _)| *cell == level)
                .count() as u32
        };
        let (good, warn, bad) = (count(Level::Good), count(Level::Warn), count(Level::Bad));
        OverlayView {
            kind,
            title,
            legend,
            cells: out
                .cells
                .into_iter()
                .map(|((y, x), (level, value))| OverlayCell { x, y, level, value })
                .collect(),
            items: out.items,
            good,
            warn,
            bad,
        }
    }

    /// Public paths are graded by guest reach from the entrance, staff-only paths by staff
    /// reach from the depot (#98); the cell value is 1 on staff-only tiles so the map can tell
    /// the two networks apart.
    fn access_overlay(&self, out: &mut Builder) {
        let guests = self.guest_reach();
        let staff = self.staff_reach();
        let mut public_cut_off = Vec::new();
        let mut staff_cut_off = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let position = Position { x, y };
                match self.path_access(position) {
                    Some(PathAccess::Public) => {
                        let reached = guests.contains(self, position);
                        if !reached && self.tile(x, y) != Some(TileKind::Entrance) {
                            public_cut_off.push(position);
                        }
                        out.cell(
                            position,
                            if reached { Level::Good } else { Level::Bad },
                            None,
                        );
                    }
                    Some(PathAccess::Staff) => {
                        let reached = staff.contains(self, position);
                        if !reached {
                            staff_cut_off.push(position);
                        }
                        let level = if reached { Level::Good } else { Level::Bad };
                        out.cell(position, level, Some(1));
                    }
                    None => {}
                }
            }
        }
        if let Some(first) = public_cut_off.first() {
            out.item(
                Level::Bad,
                Subject::Paths,
                *first,
                format!(
                    "{} public path tile(s) are not connected to the entrance",
                    public_cut_off.len()
                ),
            );
        }
        if let Some(first) = staff_cut_off.first() {
            out.item(
                Level::Bad,
                Subject::Paths,
                *first,
                format!(
                    "{} staff-only path tile(s) are not connected to the depot",
                    staff_cut_off.len()
                ),
            );
        }
        let occupancy = self.viewing_occupancy();
        for habitat in self.habitats.iter().filter(|habitat| habitat.stocked()) {
            let viewable = self
                .viewing_spots_with_occupancy(habitat, &occupancy)
                .iter()
                .any(|spot| spot.capacity > 0 && guests.contains(self, spot.position));
            let level = if viewable { Level::Good } else { Level::Bad };
            out.footprint(habitat, level, None);
            out.item(
                level,
                Subject::Habitat(habitat.id),
                Self::habitat_center(habitat),
                format!("Guests cannot see habitat #{}", habitat.id),
            );
            if !self.service_gate_reachable(habitat)
                && let Some(access) = self.gate_access_tile(habitat.service_gate)
            {
                out.cell(access, Level::Warn, None);
                out.item(
                    Level::Warn,
                    Subject::Habitat(habitat.id),
                    access,
                    format!(
                        "Habitat #{}'s service gate has no path from the depot",
                        habitat.id
                    ),
                );
            }
        }
        for stand in self
            .concessions
            .iter()
            .filter(|stand| !stand.kind.is_utility())
        {
            let position = Position {
                x: stand.x,
                y: stand.y,
            };
            let reachable = self
                .concession_customer_tiles(stand.id)
                .into_iter()
                .any(|tile| guests.contains(self, tile));
            let level = if reachable { Level::Good } else { Level::Bad };
            out.cell(position, level, None);
            out.item(
                level,
                Subject::Concession(stand.id),
                position,
                format!(
                    "Guests cannot reach {} #{}",
                    stand.definition.label, stand.id
                ),
            );
        }
    }

    fn cleanliness_overlay(&self, out: &mut Builder) {
        let now = self.absolute_minute();
        let staff = self.staff_reach();
        for y in 0..self.height {
            for x in 0..self.width {
                if matches!(self.tile(x, y), Some(TileKind::Path(_))) {
                    out.cell(Position { x, y }, Level::Good, None);
                }
            }
        }
        let mut litter: Vec<_> = self.litter.iter().collect();
        litter.sort_by_key(|task| task.created_minute);
        for task in litter {
            let position = Position {
                x: task.x,
                y: task.y,
            };
            let age = now.saturating_sub(task.created_minute);
            let neglected = self.janitors.is_empty() || !staff.contains(self, position);
            let level = if neglected { Level::Bad } else { Level::Warn };
            out.cell(position, level, Some(age.min(u64::from(u32::MAX)) as u32));
            out.item(
                level,
                Subject::Paths,
                position,
                format!(
                    "Litter at {},{} · {age} min{}",
                    task.x,
                    task.y,
                    if neglected {
                        " · nobody can clean it"
                    } else {
                        ""
                    }
                ),
            );
        }
        for habitat in self.habitats.iter().filter(|habitat| habitat.stocked()) {
            let level = grade(habitat.cleanliness, 70, 40);
            out.footprint(habitat, level, Some(habitat.cleanliness));
            out.item(
                level,
                Subject::Habitat(habitat.id),
                Self::habitat_center(habitat),
                format!(
                    "Habitat #{} cleanliness {}",
                    habitat.id, habitat.cleanliness
                ),
            );
        }
    }

    fn facilities_overlay(&self, out: &mut Builder) {
        for (index, stand) in self.concessions.iter().enumerate() {
            let position = Position {
                x: stand.x,
                y: stand.y,
            };
            // Toilets, benches and bins (#101): closed or full is bad, queues, dirt and
            // waiting janitor work warn.
            if let Some(status) = self.comfort_status(index) {
                let (_, waiting, _) = self.facility_occupancy(stand.id);
                let serviced = self
                    .facility_service
                    .iter()
                    .any(|task| task.concession_id == stand.id);
                let blocked = match stand.kind {
                    super::ConcessionKind::Bin => stand.fill >= stand.definition.capacity,
                    _ => !self.facility_accepting(index),
                };
                let level = if blocked {
                    Level::Bad
                } else if waiting > 0 || serviced {
                    Level::Warn
                } else {
                    Level::Good
                };
                let value = match stand.kind {
                    super::ConcessionKind::Toilet => stand.cleanliness,
                    super::ConcessionKind::Bin => stand.fill,
                    _ => stand.slots.len() as u32,
                };
                out.cell(position, level, Some(value));
                out.item(
                    level,
                    Subject::Concession(stand.id),
                    position,
                    format!("{} #{} · {status}", stand.definition.label, stand.id),
                );
                continue;
            }
            let state = self.concession_service_state(stand);
            let level = match state {
                "failed" => Level::Bad,
                "degraded" => Level::Warn,
                _ => Level::Good,
            };
            out.cell(position, level, Some(stand.condition));
            out.item(
                level,
                Subject::Concession(stand.id),
                position,
                format!(
                    "{} #{} {state} · {}% condition",
                    stand.definition.label, stand.id, stand.condition
                ),
            );
        }
        for (need, wanted) in [(Need::Toilet, "a toilet"), (Need::Rest, "a seat")] {
            let waiting: Vec<_> = self.unmet_guests(need).collect();
            if let Some(first) = waiting.first() {
                out.item(
                    Level::Bad,
                    Subject::Paths,
                    Position {
                        x: first.x,
                        y: first.y,
                    },
                    format!(
                        "{} guest(s) found no {wanted} they could use",
                        waiting.len()
                    ),
                );
            }
        }
        let stocked = self.habitats.iter().any(|habitat| habitat.stocked());
        let level = match self.feed_crates {
            0 if stocked => Level::Bad,
            1..=2 if stocked => Level::Warn,
            _ => Level::Good,
        };
        out.cell(self.depot, level, Some(self.feed_crates));
        out.item(
            level,
            Subject::Depot,
            self.depot,
            format!("Depot feed stock {} crate(s)", self.feed_crates),
        );
    }

    /// Utility coverage, plant capacity and habitat supply (#113).
    fn utilities_overlay(&self, out: &mut Builder) {
        // Path tiles within a plant's coverage: covered by a working plant is good, only by a
        // failed one warns.
        let mut covered: BTreeMap<(u32, u32), Level> = BTreeMap::new();
        for stand in &self.concessions {
            let Some(service) = stand.definition.utility else {
                continue;
            };
            let level = if stand.condition > super::MAINTENANCE_FAILURE_THRESHOLD {
                Level::Good
            } else {
                Level::Warn
            };
            let distances = self.utility_distances(stand.id, service.coverage_tiles);
            for y in 0..self.height {
                for x in 0..self.width {
                    if self.index(x, y).is_some_and(|i| distances[i].is_some()) {
                        let entry = covered.entry((y, x)).or_insert(level);
                        *entry = (*entry).min(level);
                    }
                }
            }
        }
        for ((y, x), level) in covered {
            out.cell(Position { x, y }, level, None);
        }
        for (index, stand) in self.concessions.iter().enumerate() {
            let Some(view) = self.utility_facility_view(index) else {
                continue;
            };
            let position = Position {
                x: stand.x,
                y: stand.y,
            };
            let level = if !view.working {
                Level::Bad
            } else if view.overloaded || self.concession_service_state(stand) == "degraded" {
                Level::Warn
            } else {
                Level::Good
            };
            out.cell(position, level, Some(view.allocated));
            out.item(
                level,
                Subject::Concession(stand.id),
                position,
                format!("{} #{} · {}", stand.definition.label, stand.id, view.status),
            );
        }
        for habitat in &self.habitats {
            let mut level = Level::Good;
            let mut worst: Option<(super::utility::UtilityKind, &super::utility::Supply)> = None;
            for kind in super::utility::UtilityKind::ALL {
                let supply = habitat.utilities.supply(kind);
                if supply.demand == 0 {
                    continue;
                }
                let this = if supply.degraded() {
                    Level::Bad
                } else if supply.short() {
                    Level::Warn
                } else {
                    Level::Good
                };
                if worst.is_none() || this > level {
                    level = this;
                    worst = Some((kind, supply));
                }
            }
            let Some((kind, supply)) = worst else {
                continue;
            };
            out.footprint(habitat, level, Some(supply.supplied));
            out.item(
                level,
                Subject::Habitat(habitat.id),
                Self::habitat_center(habitat),
                format!(
                    "Habitat #{} · {} {}/{}",
                    habitat.id,
                    kind.label().to_lowercase(),
                    supply.supplied,
                    supply.demand
                ),
            );
        }
    }

    fn viewing_overlay(&self, out: &mut Builder) {
        let occupancy = self.viewing_occupancy();
        for habitat in self.habitats.iter().filter(|habitat| habitat.stocked()) {
            for spot in self.viewing_spots_with_occupancy(habitat, &occupancy) {
                if spot.capacity == 0 {
                    continue;
                }
                let level = if spot.crowded() {
                    Level::Bad
                } else if spot.occupancy >= spot.capacity {
                    Level::Warn
                } else {
                    Level::Good
                };
                out.cell(spot.position, level, Some(spot.occupancy));
                out.item(
                    level,
                    Subject::Habitat(habitat.id),
                    spot.position,
                    format!(
                        "Habitat #{} viewpoint {},{} · {} of {} guests",
                        habitat.id, spot.position.x, spot.position.y, spot.occupancy, spot.capacity
                    ),
                );
            }
        }
        // Crowded path tiles (#103): more guests on one tile than it comfortably holds.
        let mut crowded: Vec<((u32, u32), u32)> = self
            .tile_guest_counts()
            .into_iter()
            .filter(|(_, guests)| *guests > super::visit::PATH_COMFORT_GUESTS)
            .collect();
        crowded.sort_by_key(|((x, y), _)| (*y, *x));
        for ((x, y), guests) in crowded {
            let position = Position { x, y };
            out.cell(position, Level::Bad, Some(guests));
            out.item(
                Level::Bad,
                Subject::Paths,
                position,
                format!("Crowded path {x},{y} · {guests} guests on one tile"),
            );
        }
    }
}

/// A complaint test over one guest.
type GuestTest = fn(&super::Guest) -> bool;

#[derive(Serialize)]
struct HabitatWelfare {
    habitat_id: u32,
    /// Resident species keys (#108).
    species: Vec<String>,
    animals: u32,
    welfare: u32,
}

#[derive(Serialize)]
struct ComplaintGroup {
    category: &'static str,
    guest_ids: Vec<u32>,
}

#[derive(Serialize)]
struct OpenJob {
    job_id: u32,
    habitat_id: u32,
    category: &'static str,
    state: &'static str,
}

#[derive(Serialize)]
pub(crate) struct DailySummary {
    day: u32,
    profit_cents: i64,
    income_cents: i64,
    expenses_cents: i64,
    /// People admitted at the gate today, and the current admission price (#106).
    admissions: u32,
    admissions_income_cents: i64,
    admission_price_cents: i64,
    /// The animal-weighted mean welfare of stocked habitats; `None` without animals.
    welfare: Option<u32>,
    welfare_habitats: Vec<HabitatWelfare>,
    complaints: u32,
    complaint_groups: Vec<ComplaintGroup>,
    unfinished: u32,
    open_jobs: Vec<OpenJob>,
    litter: u32,
    maintenance: u32,
    /// Open janitor work at toilets and bins (#101).
    facility_service: u32,
    /// People who walked out today, and why (#103).
    exits: u32,
    exit_reasons: super::visit::ExitCounts,
    headline: String,
}

impl GameState {
    pub(crate) fn daily_summary(&self) -> DailySummary {
        let ledger = self.finance_today;
        let price = self.pricing.admission_cents;
        let admissions = self.pricing.today.people_admitted;
        let stocked: Vec<&Habitat> = self
            .habitats
            .iter()
            .filter(|habitat| habitat.stocked())
            .collect();
        let animals: u64 = stocked
            .iter()
            .map(|habitat| u64::from(habitat.animal_count()))
            .sum();
        let welfare = (animals > 0).then(|| {
            (stocked
                .iter()
                .map(|habitat| u64::from(habitat.welfare()) * u64::from(habitat.animal_count()))
                .sum::<u64>()
                / animals) as u32
        });
        let groups: [(&'static str, GuestTest); 6] = [
            ("hungry", |guest| guest.hunger >= 60),
            ("thirsty", |guest| guest.thirst >= 60),
            ("tired", |guest| guest.energy <= 35),
            ("poor_value", |guest| guest.value_perception <= 40),
            ("needs_toilet", |guest| {
                guest.unmet_need == Some(Need::Toilet) && guest.visit.is_none()
            }),
            ("needs_rest", |guest| {
                guest.unmet_need == Some(Need::Rest) && guest.visit.is_none()
            }),
        ];
        let complaint_groups: Vec<ComplaintGroup> = groups
            .into_iter()
            .map(|(category, test)| ComplaintGroup {
                category,
                guest_ids: self
                    .guests
                    .iter()
                    .filter(|guest| test(guest))
                    .map(|guest| guest.id)
                    .collect(),
            })
            .collect();
        let complaints = complaint_groups
            .iter()
            .map(|group| group.guest_ids.len() as u32)
            .sum();
        let open_jobs: Vec<OpenJob> = self
            .care_jobs
            .iter()
            .filter_map(|job| {
                let state = match job.state {
                    CareJobState::Due => "due",
                    CareJobState::InProgress => "in_progress",
                    _ => return None,
                };
                Some(OpenJob {
                    job_id: job.id,
                    habitat_id: job.habitat_id,
                    category: job.category.key(),
                    state,
                })
            })
            .collect();
        let litter = self.litter.len() as u32;
        let maintenance = self.maintenance.len() as u32;
        let facility_service = self.facility_service.len() as u32;
        let unfinished = open_jobs.len() as u32 + litter + maintenance + facility_service;
        let profit = ledger.profit_cents();
        let critical = self
            .alerts
            .iter()
            .filter(|alert| alert.severity == super::alerts::Severity::Critical)
            .count();
        let headline = match self.alerts.len() {
            0 => None,
            1 => Some("1 problem needs attention.".to_owned()),
            count => Some(format!(
                "{count} problems need attention{}.",
                if critical > 0 {
                    format!(", {critical} critical")
                } else {
                    String::new()
                }
            )),
        };
        let headline = match headline {
            Some(headline) => headline,
            None if stocked.is_empty() => "No animals yet: build and stock a habitat.".to_owned(),
            None => format!("No urgent problems. Profit today {}.", dollars(profit)),
        };
        DailySummary {
            day: self.day,
            profit_cents: profit,
            income_cents: ledger.income_total_cents(),
            expenses_cents: ledger.expense_total_cents(),
            admissions,
            admissions_income_cents: ledger.admissions_income_cents,
            admission_price_cents: price,
            welfare,
            welfare_habitats: stocked
                .iter()
                .map(|habitat| HabitatWelfare {
                    habitat_id: habitat.id,
                    species: habitat.species_keys(),
                    animals: habitat.animal_count(),
                    welfare: habitat.welfare(),
                })
                .collect(),
            complaints,
            complaint_groups,
            unfinished,
            open_jobs,
            litter,
            maintenance,
            facility_service,
            exits: self.visits.today.total(),
            exit_reasons: self.visits.today,
            headline,
        }
    }
}
