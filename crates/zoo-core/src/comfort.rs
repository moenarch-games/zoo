//! Guest-comfort facilities (#101): toilets, benches and litter bins.
//!
//! Guests accumulate a need for a toilet and lose energy as they walk. Once a need crosses its
//! threshold a walking guest picks a facility deterministically: among the toilets (or benches)
//! that are open and whose public customer tiles the guest can reach, the one with a free slot,
//! then the shortest route, then the lowest ID. Choosing a facility with a free slot reserves
//! that slot at once, so two guests can never be given the same cubicle or seat; a guest who
//! can only reach full facilities walks to the nearest and queues there for a bounded time
//! before giving up. A slot is held from reservation until the use ends, and every way a visit
//! can end (finished, abandoned, demolished, cut off, guest leaving) releases it.
//!
//! Recovery comes only from use: a toilet empties the bladder after a short visit, and a bench
//! restores energy only for the minutes a guest actually sits on it. A guest who finds nothing
//! usable records the unmet need, which drives its thought, the complaint counts and the
//! management alert, and tries again later. Bins are used by explicit guest behaviour: a guest
//! who has just bought something drops the wrapper in a bin beside the path tile they stand on,
//! or on the path when there is none with room.
//!
//! Dirty toilets and full bins raise one janitor service task per facility, served by the
//! existing janitors through the same queue as path litter; the facility recovers only when the
//! janitor arrives at a service point. Toilets also wear like stands and are repaired by
//! mechanics. Construction and upkeep reuse the stand transactions and ledger lines.
use super::{
    ConcessionKind, GameState, Guest, GuestState, MAINTENANCE_FAILURE_THRESHOLD,
    MAINTENANCE_WEAR_PER_SALE, Mover, Position,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Bladder level of an arriving guest.
pub(crate) const ARRIVAL_BLADDER: u32 = 10;
/// Bladder need a drink adds.
pub(crate) const DRINK_BLADDER: u32 = 15;
/// From this bladder level a guest looks for a toilet.
pub(crate) const TOILET_SEEK_BLADDER: u32 = 60;
/// From this level a guest is unhappy and will not start another habitat.
pub(crate) const BLADDER_DESPERATE: u32 = 90;
/// At or below this energy a guest looks for a bench.
pub(crate) const REST_SEEK_ENERGY: u32 = 70;
/// A resting guest stands up at this energy.
const RESTED_ENERGY: u32 = 90;
const BENCH_ENERGY_PER_MINUTE: u32 = 2;
const BENCH_MAX_MINUTES: u32 = 20;
const TOILET_USE_MINUTES: u32 = 4;
/// How long a guest queues at a full facility before giving up.
pub(crate) const WAIT_LIMIT_MINUTES: u32 = 12;
/// How long a guest who found nothing waits before looking again.
pub(crate) const NEED_RETRY_MINUTES: u64 = 10;
const ABANDON_HAPPINESS: u32 = 3;
/// Cleanliness one toilet visit costs.
const TOILET_SOIL_PER_USE: u32 = 15;
/// At or below this cleanliness a toilet asks for a janitor.
pub(crate) const TOILET_SERVICE_CLEANLINESS: u32 = 55;
/// At or below this cleanliness a toilet is closed until cleaned.
pub(crate) const TOILET_CLOSED_CLEANLINESS: u32 = 10;

/// A guest need a comfort facility meets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Need {
    Toilet,
    Rest,
    /// Met by a food stand (#102).
    Food,
    /// Met by a drink stand (#102).
    Drink,
}

impl Need {
    fn facility(self) -> ConcessionKind {
        match self {
            Self::Toilet => ConcessionKind::Toilet,
            Self::Rest => ConcessionKind::Bench,
            Self::Food => ConcessionKind::Food,
            Self::Drink => ConcessionKind::Drink,
        }
    }
}

/// What the guest returns to once the visit ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Resume {
    Habitat,
    Exit,
}

/// A guest's errand to one comfort facility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Visit {
    pub facility_id: u32,
    pub need: Need,
    /// Queueing: minutes left before giving up. Using: minutes spent so far.
    pub minutes: u32,
    pub resume: Resume,
}

/// Janitor work at one toilet or bin: at most one open task per facility.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceTask {
    pub id: u32,
    pub concession_id: u32,
    pub created_minute: u64,
    pub assigned_janitor_id: Option<u32>,
}

/// A toilet, bench or bin as the inspector and overlays show it.
#[derive(Serialize)]
pub(crate) struct ComfortView {
    /// `toilet`, `bench` or `bin`.
    service: &'static str,
    capacity: u32,
    /// Guests using a slot now.
    in_use: u32,
    /// Guests queueing for a slot.
    waiting: u32,
    /// Guests on their way, with or without a reserved slot.
    heading: u32,
    /// Whether it takes new guests (toilets close when broken down or too dirty).
    open: bool,
    /// Toilets only.
    cleanliness: Option<u32>,
    /// Bins only: litter held.
    fill: Option<u32>,
    total_uses: u32,
    status: String,
    /// Toilets and bins: who services it, or that it needs nothing.
    janitor_status: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct GuestComfortView {
    pub bladder: u32,
    pub facility_id: Option<u32>,
    pub unmet_need: Option<Need>,
}

#[derive(Serialize)]
pub(crate) struct ComfortOperationsView {
    pub facility_service_backlog: u32,
    pub oldest_facility_service_age_minutes: u64,
    /// Guests who found no usable toilet or bench on their last try.
    pub toilet_demand_unmet: u32,
    pub rest_demand_unmet: u32,
}

#[derive(Serialize)]
pub(crate) struct ServiceTaskView {
    id: u32,
    concession_id: u32,
    /// `clean_toilet` or `empty_bin`.
    work: &'static str,
    x: u32,
    y: u32,
    age_minutes: u64,
    assigned_janitor_id: Option<u32>,
    status: String,
}

fn guest_position(guest: &Guest) -> Position {
    Position {
        x: guest.x,
        y: guest.y,
    }
}

/// Needs a guest wants met now, most urgent first.
fn needs(guest: &Guest) -> Vec<Need> {
    let mut needs = Vec::new();
    if guest.bladder >= TOILET_SEEK_BLADDER {
        needs.push(Need::Toilet);
    }
    if guest.thirst >= super::stand::DRINK_SEEK_THIRST {
        needs.push(Need::Drink);
    }
    if guest.hunger >= super::stand::FOOD_SEEK_HUNGER {
        needs.push(Need::Food);
    }
    if guest.energy <= REST_SEEK_ENERGY {
        needs.push(Need::Rest);
    }
    needs
}

impl GameState {
    fn facility_index(&self, id: u32) -> Option<usize> {
        self.concessions.iter().position(|stand| stand.id == id)
    }

    /// Whether a toilet or bench takes new guests: benches always, toilets unless broken down or
    /// too dirty.
    pub(crate) fn facility_accepting(&self, index: usize) -> bool {
        let stand = &self.concessions[index];
        match stand.kind {
            ConcessionKind::Toilet => {
                stand.condition > MAINTENANCE_FAILURE_THRESHOLD
                    && stand.cleanliness > TOILET_CLOSED_CLEANLINESS
            }
            ConcessionKind::Bench => true,
            _ => false,
        }
    }

    fn has_free_slot(&self, index: usize) -> bool {
        let stand = &self.concessions[index];
        (stand.slots.len() as u32) < stand.definition.capacity
    }

    /// The facility a guest at `start` should use for `need`, its route and whether a slot is
    /// free: free slots first, then the shortest route, then the lowest ID.
    fn pick_facility(&self, need: Need, start: Position) -> Option<(usize, Vec<Position>, bool)> {
        /// Full last, then route length, then ID.
        type Rank = (bool, usize, u32);
        let mut best: Option<(Rank, usize, Vec<Position>)> = None;
        for index in 0..self.concessions.len() {
            let stand = &self.concessions[index];
            if stand.kind != need.facility() || !self.facility_accepting(index) {
                continue;
            }
            let goals = self.concession_customer_tiles(stand.id);
            let Some(route) = self.path_to_any(Mover::Guest, start, &goals) else {
                continue;
            };
            let free = self.has_free_slot(index);
            let key = (!free, route.len(), stand.id);
            if best.as_ref().is_none_or(|(best_key, ..)| key < *best_key) {
                best = Some((key, index, route));
            }
        }
        best.map(|((full, ..), index, route)| (index, route, !full))
    }

    /// Walking guests whose need crossed its threshold set off for a facility (#101) or a
    /// stand (#102), most urgent need first. A need no stand could meet is a lost sale for the
    /// nearest stand that turned the guest away.
    pub(crate) fn choose_comfort_visits(&mut self) {
        let now = self.absolute_minute();
        let mut heading: Option<Vec<u32>> = None;
        for index in 0..self.guests.len() {
            let guest = &self.guests[index];
            let resume = match guest.state {
                GuestState::WalkingToHabitat => Resume::Habitat,
                GuestState::WalkingToExit => Resume::Exit,
                _ => continue,
            };
            if guest.next_need_check > now {
                continue;
            }
            let wanted = needs(guest);
            let Some(&first) = wanted.first() else {
                continue;
            };
            let start = guest_position(guest);
            let cash = guest.cash_cents;
            let mut choice = None;
            for need in wanted.iter().copied() {
                if need.stand().is_some() {
                    let counts = heading.get_or_insert_with(|| self.stand_heading_counts());
                    match self.pick_stand(need, start, cash, counts) {
                        Ok((stand, route)) => {
                            counts[stand] += 1;
                            self.start_stand_visit(index, stand, need, route, resume);
                            choice = Some(None);
                            break;
                        }
                        Err(refused) => self.refuse_stand(refused),
                    }
                } else if let Some((facility, route, free)) = self.pick_facility(need, start) {
                    choice = Some(Some((need, facility, route, free)));
                    break;
                }
            }
            let Some(choice) = choice else {
                let guest = &mut self.guests[index];
                guest.unmet_need = Some(first);
                guest.next_need_check = now + NEED_RETRY_MINUTES;
                continue;
            };
            // A stand visit has already begun.
            let Some((need, facility, route, free)) = choice else {
                continue;
            };
            let guest_id = self.guests[index].id;
            if free {
                self.concessions[facility].slots.push(guest_id);
            }
            let guest = &mut self.guests[index];
            guest.visit = Some(Visit {
                facility_id: self.concessions[facility].id,
                need,
                minutes: 0,
                resume,
            });
            guest.state = GuestState::WalkingToFacility;
            guest.route = route;
            guest.route_index = 0;
        }
    }

    /// A guest reached the end of its route to a facility: use a held or free slot, or queue.
    pub(crate) fn arrive_at_facility(&mut self, index: usize) {
        let guest_id = self.guests[index].id;
        let Some(visit) = self.guests[index].visit else {
            self.resume_walk(index, Resume::Exit);
            return;
        };
        if visit.need.stand().is_some() {
            self.arrive_at_stand(index);
            return;
        }
        let Some(facility) = self.facility_index(visit.facility_id) else {
            self.end_visit(index, true);
            return;
        };
        if !self.facility_accepting(facility) {
            self.end_visit(index, true);
            return;
        }
        let holds = self.concessions[facility].slots.contains(&guest_id);
        let guest = &mut self.guests[index];
        guest.route.clear();
        guest.route_index = 0;
        if holds || self.has_free_slot(facility) {
            if !holds {
                self.concessions[facility].slots.push(guest_id);
            }
            self.start_use(index);
        } else {
            let guest = &mut self.guests[index];
            guest.state = GuestState::WaitingForFacility;
            if let Some(visit) = guest.visit.as_mut() {
                visit.minutes = WAIT_LIMIT_MINUTES;
            }
        }
    }

    fn start_use(&mut self, index: usize) {
        let guest = &mut self.guests[index];
        guest.state = GuestState::UsingFacility;
        if let Some(visit) = guest.visit.as_mut() {
            visit.minutes = 0;
        }
    }

    /// One simulated minute of queueing and use.
    pub(crate) fn advance_comfort_visits(&mut self) {
        for index in 0..self.guests.len() {
            let guest = &self.guests[index];
            // Stands run their own queues and windows (#102).
            let Some(visit) = guest.visit.filter(|visit| visit.need.stand().is_none()) else {
                continue;
            };
            match guest.state {
                GuestState::WaitingForFacility => {
                    let position = guest_position(guest);
                    let facility = self
                        .facility_index(visit.facility_id)
                        .filter(|facility| self.facility_accepting(*facility))
                        .filter(|facility| {
                            self.concession_customer_tiles(self.concessions[*facility].id)
                                .contains(&position)
                        });
                    let Some(facility) = facility else {
                        self.end_visit(index, true);
                        continue;
                    };
                    if self.has_free_slot(facility) {
                        let guest_id = self.guests[index].id;
                        self.concessions[facility].slots.push(guest_id);
                        self.start_use(index);
                        continue;
                    }
                    let remaining = visit.minutes.saturating_sub(1);
                    if remaining == 0 {
                        self.end_visit(index, true);
                    } else if let Some(visit) = self.guests[index].visit.as_mut() {
                        visit.minutes = remaining;
                    }
                }
                GuestState::UsingFacility => {
                    let minutes = visit.minutes.saturating_add(1);
                    let guest = &mut self.guests[index];
                    if let Some(visit) = guest.visit.as_mut() {
                        visit.minutes = minutes;
                    }
                    let done = match visit.need {
                        Need::Toilet => minutes >= TOILET_USE_MINUTES,
                        Need::Rest => {
                            guest.energy = guest
                                .energy
                                .saturating_add(BENCH_ENERGY_PER_MINUTE)
                                .min(100);
                            guest.energy >= RESTED_ENERGY || minutes >= BENCH_MAX_MINUTES
                        }
                        Need::Food | Need::Drink => false,
                    };
                    if done {
                        self.finish_use(index);
                    }
                }
                _ => {}
            }
        }
    }

    fn finish_use(&mut self, index: usize) {
        let now = self.absolute_minute();
        let Some(visit) = self.guests[index].visit.take() else {
            return;
        };
        let guest_id = self.guests[index].id;
        self.release_facility_slot(guest_id);
        if let Some(facility) = self.facility_index(visit.facility_id) {
            let stand = &mut self.concessions[facility];
            stand.total_uses = stand.total_uses.saturating_add(1);
            if visit.need == Need::Toilet {
                stand.cleanliness = stand.cleanliness.saturating_sub(TOILET_SOIL_PER_USE);
                stand.condition = stand.condition.saturating_sub(MAINTENANCE_WEAR_PER_SALE);
                self.ensure_maintenance_task(visit.facility_id);
                self.ensure_facility_service(visit.facility_id);
            }
        }
        let guest = &mut self.guests[index];
        match visit.need {
            Need::Toilet => {
                guest.bladder = 0;
                guest.happiness = guest.happiness.saturating_add(3).min(100);
            }
            Need::Rest => guest.happiness = guest.happiness.saturating_add(2).min(100),
            // Stand sales complete in `stand::complete_sale`.
            Need::Food | Need::Drink => {}
        }
        if guest.unmet_need == Some(visit.need) {
            guest.unmet_need = None;
        }
        guest.next_need_check = now + NEED_RETRY_MINUTES;
        self.resume_walk(index, visit.resume);
    }

    /// Ends a visit without use. An unmet end is a failed attempt: the guest remembers the need,
    /// is a little unhappier and waits before trying again; otherwise (the facility was removed)
    /// the guest may choose another at once.
    pub(crate) fn end_visit(&mut self, index: usize, unmet: bool) {
        let now = self.absolute_minute();
        let guest_id = self.guests[index].id;
        let Some(visit) = self.guests[index].visit.take() else {
            return;
        };
        self.release_facility_slot(guest_id);
        let guest = &mut self.guests[index];
        if unmet {
            guest.unmet_need = Some(visit.need);
            guest.happiness = guest.happiness.saturating_sub(ABANDON_HAPPINESS);
            guest.next_need_check = now + NEED_RETRY_MINUTES;
        } else {
            guest.next_need_check = now;
        }
        self.resume_walk(index, visit.resume);
    }

    /// Sends a guest back to its habitat plan, another habitat, or the exit.
    pub(crate) fn resume_walk(&mut self, index: usize, resume: Resume) {
        let occupancy = self.viewing_occupancy();
        let guest = &self.guests[index];
        let start = guest_position(guest);
        let habitat = match resume {
            Resume::Exit => None,
            Resume::Habitat => self
                .habitats
                .iter()
                .find(|habitat| habitat.id == guest.target_habitat && habitat.stocked())
                .filter(|_| !guest.visited_habitats.contains(&guest.target_habitat))
                .and_then(|habitat| {
                    self.viewing_route_with_occupancy(habitat, start, &occupancy)
                        .map(|route| (habitat.id, route))
                })
                .or_else(|| {
                    guest
                        .wants_another_habitat()
                        .then(|| self.next_engaging_habitat(guest, start, &occupancy))
                        .flatten()
                }),
        };
        let exit = self.entrance;
        let (state, target, route) = match habitat {
            Some((habitat_id, route)) => (GuestState::WalkingToHabitat, Some(habitat_id), route),
            None => (
                GuestState::WalkingToExit,
                None,
                self.path_between(Mover::Guest, start, exit)
                    .unwrap_or_default(),
            ),
        };
        let guest = &mut self.guests[index];
        guest.state = state;
        if let Some(target) = target {
            guest.target_habitat = target;
        }
        guest.route = route;
        guest.route_index = 0;
    }

    /// A path edit cut a guest's walk to a facility: route again, or give up on it.
    pub(crate) fn reroute_facility_visit(&mut self, index: usize) {
        let guest = &self.guests[index];
        let start = guest_position(guest);
        let route = guest
            .visit
            .and_then(|visit| self.facility_index(visit.facility_id))
            .filter(|facility| {
                if self.concessions[*facility].stand.is_some() {
                    self.stand_availability(*facility).takes_guests()
                } else {
                    self.facility_accepting(*facility)
                }
            })
            .and_then(|facility| {
                let goals = self.concession_customer_tiles(self.concessions[facility].id);
                self.path_to_any(Mover::Guest, start, &goals)
            });
        match route {
            Some(route) => {
                let guest = &mut self.guests[index];
                guest.route = route;
                guest.route_index = 0;
            }
            None => self.end_visit(index, true),
        }
    }

    /// Frees whatever slot or stand queue place a guest holds.
    pub(crate) fn release_facility_slot(&mut self, guest_id: u32) {
        for stand in &mut self.concessions {
            stand.slots.retain(|holder| *holder != guest_id);
        }
        self.release_stand_queue(guest_id);
    }

    /// Whether any guest is walking to, queueing at or using a facility.
    pub(crate) fn facility_in_use(&self, concession_id: u32) -> bool {
        self.guests.iter().any(|guest| {
            guest
                .visit
                .is_some_and(|visit| visit.facility_id == concession_id)
        })
    }

    /// Before a facility is demolished: every guest bound for it chooses again, and its janitor
    /// work is cancelled.
    pub(crate) fn release_facility(&mut self, concession_id: u32) {
        for index in 0..self.guests.len() {
            if self.guests[index]
                .visit
                .is_some_and(|visit| visit.facility_id == concession_id)
            {
                self.end_visit(index, false);
            }
        }
        let cancelled: Vec<u32> = self
            .facility_service
            .iter()
            .filter(|task| task.concession_id == concession_id)
            .map(|task| task.id)
            .collect();
        self.facility_service
            .retain(|task| task.concession_id != concession_id);
        for janitor in &mut self.janitors {
            if janitor.target.is_some_and(|target| {
                matches!(target, super::JanitorTarget::Facility(id) if cancelled.contains(&id))
            }) {
                janitor.target = None;
            }
        }
    }

    /// A guest who just bought something drops the wrapper in a bin with room beside the tile
    /// they stand on (lowest ID first), or on the path when there is none.
    pub(crate) fn dispose_litter(&mut self, position: Position) {
        let bin = self
            .concessions
            .iter()
            .filter(|stand| stand.kind == ConcessionKind::Bin)
            .filter(|stand| stand.fill < stand.definition.capacity)
            .find(|stand| {
                self.concession_customer_tiles(stand.id)
                    .iter()
                    .any(|tile| tile.x.abs_diff(position.x) + tile.y.abs_diff(position.y) <= 1)
            })
            .map(|stand| stand.id);
        let Some(bin_id) = bin else {
            self.add_litter(position);
            return;
        };
        if let Some(index) = self.facility_index(bin_id) {
            let stand = &mut self.concessions[index];
            stand.fill += 1;
            stand.total_uses = stand.total_uses.saturating_add(1);
        }
        self.ensure_facility_service(bin_id);
    }

    /// Whether a toilet or bin needs a janitor now.
    pub(crate) fn facility_needs_service(&self, index: usize) -> bool {
        let stand = &self.concessions[index];
        match stand.kind {
            ConcessionKind::Toilet => stand.cleanliness <= TOILET_SERVICE_CLEANLINESS,
            ConcessionKind::Bin => stand.fill >= stand.definition.capacity,
            _ => false,
        }
    }

    /// Raises the facility's janitor task once per actual problem.
    pub(crate) fn ensure_facility_service(&mut self, concession_id: u32) -> bool {
        let Some(index) = self.facility_index(concession_id) else {
            return false;
        };
        if !self.facility_needs_service(index)
            || self
                .facility_service
                .iter()
                .any(|task| task.concession_id == concession_id)
        {
            return false;
        }
        let id = self.next_facility_service_id;
        self.next_facility_service_id = self.next_facility_service_id.saturating_add(1);
        self.facility_service.push(ServiceTask {
            id,
            concession_id,
            created_minute: self.absolute_minute(),
            assigned_janitor_id: None,
        });
        true
    }

    /// A janitor arrived at the facility: the toilet is clean or the bin empty again.
    pub(crate) fn complete_facility_service(&mut self, task_id: u32) {
        let Some(task) = self
            .facility_service
            .iter()
            .position(|task| task.id == task_id)
        else {
            return;
        };
        let task = self.facility_service.remove(task);
        if let Some(index) = self.facility_index(task.concession_id) {
            let stand = &mut self.concessions[index];
            stand.cleanliness = 100;
            stand.fill = 0;
        }
    }

    pub(crate) fn oldest_facility_service_age_minutes(&self) -> u64 {
        let now = self.absolute_minute();
        self.facility_service
            .iter()
            .map(|task| now.saturating_sub(task.created_minute))
            .max()
            .unwrap_or(0)
    }

    /// Guests whose need the park failed to meet and who are not on their way to it now.
    pub(crate) fn unmet_guests(&self, need: Need) -> impl Iterator<Item = &Guest> {
        self.guests
            .iter()
            .filter(move |guest| guest.unmet_need == Some(need) && guest.visit.is_none())
    }

    /// Guests on their way to (`reserved`), queueing at or using a facility.
    pub(crate) fn facility_occupancy(&self, concession_id: u32) -> (u32, u32, u32) {
        let mut counts = (0, 0, 0);
        for guest in &self.guests {
            if !guest
                .visit
                .is_some_and(|visit| visit.facility_id == concession_id)
            {
                continue;
            }
            match guest.state {
                GuestState::UsingFacility => counts.0 += 1,
                GuestState::WaitingForFacility => counts.1 += 1,
                _ => counts.2 += 1,
            }
        }
        counts
    }

    /// One line on what a comfort facility offers right now; `None` for stands.
    pub(crate) fn comfort_status(&self, index: usize) -> Option<String> {
        let stand = &self.concessions[index];
        let capacity = stand.definition.capacity;
        let (using, waiting, _) = self.facility_occupancy(stand.id);
        let queue = if waiting > 0 {
            format!(" · {waiting} waiting")
        } else {
            String::new()
        };
        Some(match stand.kind {
            ConcessionKind::Toilet if stand.condition <= MAINTENANCE_FAILURE_THRESHOLD => {
                "Closed · broken down".to_owned()
            }
            ConcessionKind::Toilet if stand.cleanliness <= TOILET_CLOSED_CLEANLINESS => {
                "Closed · too dirty to use".to_owned()
            }
            ConcessionKind::Toilet => format!("Open · {using}/{capacity} in use{queue}"),
            ConcessionKind::Bench => format!("{using}/{capacity} seats taken{queue}"),
            ConcessionKind::Bin if stand.fill >= capacity => {
                "Full · new litter lands on the path".to_owned()
            }
            ConcessionKind::Bin => format!("{}/{capacity} full", stand.fill),
            ConcessionKind::Food | ConcessionKind::Drink => return None,
        })
    }

    pub(crate) fn comfort_view(&self, index: usize) -> Option<ComfortView> {
        let stand = &self.concessions[index];
        let status = self.comfort_status(index)?;
        let (in_use, waiting, heading) = self.facility_occupancy(stand.id);
        let janitor_status = matches!(stand.kind, ConcessionKind::Toilet | ConcessionKind::Bin)
            .then(|| {
                self.facility_service
                    .iter()
                    .find(|task| task.concession_id == stand.id)
                    .map_or_else(
                        || "No service needed".to_owned(),
                        |task| self.facility_service_status(task),
                    )
            });
        Some(ComfortView {
            service: stand.kind.key(),
            capacity: stand.definition.capacity,
            in_use,
            waiting,
            heading,
            open: stand.kind == ConcessionKind::Bin || self.facility_accepting(index),
            cleanliness: (stand.kind == ConcessionKind::Toilet).then_some(stand.cleanliness),
            fill: (stand.kind == ConcessionKind::Bin).then_some(stand.fill),
            total_uses: stand.total_uses,
            status,
            janitor_status,
        })
    }

    pub(crate) fn facility_service_views(&self) -> Vec<ServiceTaskView> {
        let now = self.absolute_minute();
        self.facility_service
            .iter()
            .filter_map(|task| {
                let stand = self
                    .concessions
                    .iter()
                    .find(|stand| stand.id == task.concession_id)?;
                Some(ServiceTaskView {
                    id: task.id,
                    concession_id: task.concession_id,
                    work: if stand.kind == ConcessionKind::Bin {
                        "empty_bin"
                    } else {
                        "clean_toilet"
                    },
                    x: stand.x,
                    y: stand.y,
                    age_minutes: now.saturating_sub(task.created_minute),
                    assigned_janitor_id: task.assigned_janitor_id,
                    status: self.facility_service_status(task),
                })
            })
            .collect()
    }

    /// Who will service a toilet or bin, for the inspector and task list.
    pub(crate) fn facility_service_status(&self, task: &ServiceTask) -> String {
        if let Some(janitor_id) = task.assigned_janitor_id {
            return format!("Janitor #{janitor_id} responding");
        }
        if self.janitors.is_empty() {
            return "Waiting · no janitor hired".to_owned();
        }
        // Presentation: an uncounted search, so a snapshot never adds route work.
        let points = self.concession_service_tiles(task.concession_id);
        let reachable = self.janitors.iter().any(|janitor| {
            self.search_path(
                Mover::Staff,
                Position {
                    x: janitor.x,
                    y: janitor.y,
                },
                &points,
            )
            .is_some()
        });
        if reachable {
            "Waiting for an available janitor".to_owned()
        } else {
            "Blocked · no janitor can reach it".to_owned()
        }
    }

    /// Reservation, visit and service-task consistency (#101), checked with the other per-tick
    /// invariants and on load.
    pub(crate) fn comfort_violation(&self) -> Option<String> {
        let guests: HashSet<u32> = self.guests.iter().map(|guest| guest.id).collect();
        let mut held = HashSet::new();
        for stand in &self.concessions {
            // A stand's windows are checked with its queue (#102, `stand_violation`).
            if stand.kind.sells() {
                continue;
            }
            let comfort = matches!(stand.kind, ConcessionKind::Toilet | ConcessionKind::Bench);
            if !comfort && !stand.slots.is_empty() {
                return Some(format!("facility {} has slots it cannot offer", stand.id));
            }
            if stand.slots.len() as u32 > stand.definition.capacity {
                return Some(format!("facility {} is over capacity", stand.id));
            }
            if stand.cleanliness > 100 || stand.fill > stand.definition.capacity {
                return Some(format!("facility {} state is out of range", stand.id));
            }
            for holder in &stand.slots {
                if !held.insert(*holder) {
                    return Some(format!("guest {holder} holds two facility slots"));
                }
                let bound = guests.contains(holder)
                    && self.guests.iter().any(|guest| {
                        guest.id == *holder
                            && matches!(
                                guest.state,
                                GuestState::WalkingToFacility | GuestState::UsingFacility
                            )
                            && guest
                                .visit
                                .is_some_and(|visit| visit.facility_id == stand.id)
                    });
                if !bound {
                    return Some(format!(
                        "facility {} holds a slot for guest {holder} who is not on its way or using it",
                        stand.id
                    ));
                }
            }
        }
        for guest in &self.guests {
            if guest.visit.is_some() != guest.on_errand() {
                return Some(format!("guest {} visit does not match its state", guest.id));
            }
            let Some(visit) = guest.visit else {
                continue;
            };
            let Some(stand) = self
                .concessions
                .iter()
                .find(|stand| stand.id == visit.facility_id)
            else {
                return Some(format!("guest {} visits a missing facility", guest.id));
            };
            if stand.kind != visit.need.facility() {
                return Some(format!(
                    "guest {} visits a facility that does not meet its need",
                    guest.id
                ));
            }
            let holds = stand.slots.contains(&guest.id);
            if (guest.state == GuestState::UsingFacility && !holds)
                || (guest.state == GuestState::WaitingForFacility && holds)
            {
                return Some(format!("guest {} slot does not match its state", guest.id));
            }
            if guest.bladder > 100 {
                return Some(format!("guest {} bladder is out of range", guest.id));
            }
        }
        let mut serviced = HashSet::new();
        let mut previous = 0;
        for task in &self.facility_service {
            if task.id <= previous || task.id >= self.next_facility_service_id {
                return Some(format!(
                    "facility service id {} is duplicated, out of order or not yet allocated",
                    task.id
                ));
            }
            previous = task.id;
            let Some(stand) = self
                .concessions
                .iter()
                .find(|stand| stand.id == task.concession_id)
            else {
                return Some(format!(
                    "facility service {} belongs to a missing facility",
                    task.id
                ));
            };
            if !matches!(stand.kind, ConcessionKind::Toilet | ConcessionKind::Bin)
                || !serviced.insert(task.concession_id)
            {
                return Some(format!(
                    "facility {} has a duplicate or unsupported service task",
                    stand.id
                ));
            }
            if let Some(janitor_id) = task.assigned_janitor_id
                && !self.janitors.iter().any(|janitor| {
                    janitor.id == janitor_id
                        && janitor.target == Some(super::JanitorTarget::Facility(task.id))
                })
            {
                return Some(format!(
                    "facility service {} is assigned a janitor working elsewhere",
                    task.id
                ));
            }
        }
        None
    }
}
