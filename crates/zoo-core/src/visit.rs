//! Visitor profiles, groups, itineraries, crowding and exit reasons (#103).
//!
//! Guests arrive in groups. A group's profile, drawn from its stable ID, fixes its size, how many
//! habitats it hopes to see, how far it will walk to the next one, how much crowding it puts up
//! with and how strongly it seeks its favourite species (one of the species on show when it
//! arrives). Admission is one ledger line per group for every person in it; every person is a
//! real guest record, so guest counts never include anyone who did not arrive.
//!
//! The group decides together. A member who finishes watching a habitat waits at its viewpoint
//! until the whole group is ready, for at most [`GROUP_WAIT_MINUTES`]; members who are still away
//! then (queueing at a toilet, cut off by a path edit) leave the group and carry on alone with
//! the same profile, so a straggler never holds anyone up indefinitely and nobody is moved
//! without walking. The decision is a small scoring rule over the stocked, engaging habitats
//! the group has not seen: interest (favourite species, appeal, welfare) minus the walk to the
//! best viewpoint (from one breadth-first walk) minus a penalty when every viewpoint is full.
//! Habitats beyond the profile's walking tolerance are skipped. Needs, crowding and value can
//! end the visit first; each member who leaves keeps the reason and the state behind it, and
//! the park counts every person's exit reason when they walk out of the gate.
//!
//! Crowding is a bounded game rule: a guest standing on a path tile shared by more than
//! [`PATH_COMFORT_GUESTS`] guests, or watching from a viewpoint over its #81 capacity, gains a
//! crowded minute. Patience is the number of crowded minutes a profile tolerates.
use super::{
    GUEST_CONTINUE_MAX_HUNGER, GUEST_CONTINUE_MAX_THIRST, GUEST_CONTINUE_MIN_ENERGY,
    GUEST_CONTINUE_MIN_HAPPINESS, GUEST_CONTINUE_MIN_VALUE, GameState, Guest, GuestState, Habitat,
    MIN_ENGAGING_HABITAT_WELFARE, Mover, Position, ViewingOccupancy, ViewingSpot, animal, comfort,
    pricing::{self, GateOutcome},
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};

/// The longest a ready member waits for the rest of its group.
pub(crate) const GROUP_WAIT_MINUTES: u32 = 15;
/// How long a member who reaches a habitat its waiting group has already watched looks for.
pub(crate) const CATCH_UP_VIEWING_MINUTES: u32 = 5;
/// More guests than this on one tile make it crowded.
pub(crate) const PATH_COMFORT_GUESTS: u32 = 6;
/// Crowded minutes are counted up to this bound.
const MAX_CROWD_MINUTES: u32 = 600;
/// Simulated minutes of arrivals per admitted person; a group of three takes three intervals.
pub(crate) const ARRIVAL_MINUTES_PER_GUEST: u32 = 24;
/// The exits the park remembers for its reports.
const RECENT_EXITS: usize = 8;

/// A visitor profile: a small, explicit set of preferences (#103).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Profile {
    /// Three or four people who see a couple of nearby habitats and dislike crowds.
    Family,
    /// One or two people who walk far for their favourite species and many habitats.
    Enthusiast,
    /// A pair who wander through a few habitats at an easy pace.
    Stroller,
}

pub(crate) struct ProfileRules {
    pub min_size: u32,
    pub max_size: u32,
    /// Habitats the group hopes to see.
    pub visit_goal: u32,
    /// The longest walk, in tiles, the group takes to its next habitat.
    pub walking_tolerance: u32,
    /// Crowded minutes a member tolerates before leaving.
    pub patience: u32,
    /// Score for a habitat of the favourite species.
    pub favorite_weight: i64,
    /// Score lost per tile walked.
    pub walk_weight: i64,
    /// Score lost when every viewpoint of a habitat is full.
    pub crowd_weight: i64,
    /// The most the group pays, in percent of what it thinks the park (or a stand's item) is
    /// worth, before its own seeded tolerance (#106).
    pub price_tolerance: u32,
}

impl Profile {
    const ALL: [Self; 3] = [Self::Family, Self::Enthusiast, Self::Stroller];

    pub(crate) fn rules(self) -> ProfileRules {
        match self {
            Self::Family => ProfileRules {
                min_size: 3,
                max_size: 4,
                visit_goal: 2,
                walking_tolerance: 24,
                patience: 20,
                favorite_weight: 10,
                walk_weight: 3,
                crowd_weight: 30,
                price_tolerance: 100,
            },
            Self::Enthusiast => ProfileRules {
                min_size: 1,
                max_size: 2,
                visit_goal: 4,
                walking_tolerance: 60,
                patience: 40,
                favorite_weight: 60,
                walk_weight: 1,
                crowd_weight: 10,
                price_tolerance: 130,
            },
            Self::Stroller => ProfileRules {
                min_size: 2,
                max_size: 2,
                visit_goal: 3,
                walking_tolerance: 40,
                patience: 30,
                favorite_weight: 20,
                walk_weight: 2,
                crowd_weight: 20,
                price_tolerance: 110,
            },
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Family => "Family",
            Self::Enthusiast => "Animal enthusiast",
            Self::Stroller => "Strollers",
        }
    }
}

/// The profile and size of the park's `arrival`-th group (counting from 1). Arrivals are drawn
/// from their own sequence, so splitting groups never changes who comes next.
pub(crate) fn group_plan(arrival: u32) -> (Profile, u32) {
    let seed = animal::mix(arrival, 0x6752_4f55);
    let profile = Profile::ALL[(seed % 3) as usize];
    let rules = profile.rules();
    let spread = rules.max_size - rules.min_size + 1;
    (profile, rules.min_size + (seed >> 8) % spread)
}

/// Which of `exhibited` (sorted species keys) the `arrival`-th group likes best.
pub(crate) fn favorite_index(arrival: u32, exhibited: usize) -> usize {
    (animal::mix(arrival, 0x5045_5453) as usize) % exhibited
}

/// The first arrival from `from` that draws a group of `size`; tests use it to stage arrivals.
#[cfg(test)]
pub(crate) fn arrival_of_size(size: u32, from: u32) -> u32 {
    (from..)
        .find(|arrival| group_plan(*arrival).1 == size)
        .expect("every size recurs")
}

#[cfg(test)]
impl VisitRecord {
    /// A record whose next group is the `arrival`-th.
    pub(crate) fn before(arrival: u32) -> Self {
        Self {
            groups_arrived: arrival - 1,
            ..Self::default()
        }
    }
}

/// Guests who arrived and decide together.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Group {
    pub id: u32,
    pub profile: Profile,
    /// The species the group most wants to see, if any was on show when it arrived.
    pub favorite_species: Option<String>,
    /// People admitted with this group; zero for a member who split off.
    pub admitted: u32,
    /// The guests still with the group, lowest ID first.
    pub members: Vec<u32>,
    /// Minutes the ready members have waited for the others.
    pub waiting_minutes: u32,
    /// Why the group chose its current destination.
    pub plan: String,
}

/// Why a guest left the park.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExitReason {
    Satisfied,
    NeedsUnmet,
    TooCrowded,
    PoorValue,
    NoReachableAttraction,
}

/// A decided exit: the reason and the state behind it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExitCause {
    pub reason: ExitReason,
    pub detail: String,
}

/// People who left, by reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExitCounts {
    pub satisfied: u32,
    pub needs_unmet: u32,
    pub too_crowded: u32,
    pub poor_value: u32,
    pub no_reachable_attraction: u32,
}

impl ExitCounts {
    fn add(&mut self, reason: ExitReason) {
        let slot = match reason {
            ExitReason::Satisfied => &mut self.satisfied,
            ExitReason::NeedsUnmet => &mut self.needs_unmet,
            ExitReason::TooCrowded => &mut self.too_crowded,
            ExitReason::PoorValue => &mut self.poor_value,
            ExitReason::NoReachableAttraction => &mut self.no_reachable_attraction,
        };
        *slot = slot.saturating_add(1);
    }

    pub(crate) fn total(self) -> u32 {
        self.satisfied
            .saturating_add(self.needs_unmet)
            .saturating_add(self.too_crowded)
            .saturating_add(self.poor_value)
            .saturating_add(self.no_reachable_attraction)
    }
}

/// One person's exit, kept for the reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExitNote {
    pub guest_id: u32,
    pub group_id: u32,
    pub minute: u64,
    pub reason: ExitReason,
    pub detail: String,
}

/// The park's visit record: arrivals and exit reasons.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VisitRecord {
    pub groups_arrived: u32,
    pub today: ExitCounts,
    pub total: ExitCounts,
    /// The latest exits, newest last.
    pub recent: Vec<ExitNote>,
}

/// One breadth-first walk from a start tile: the distance and parent of every reachable tile.
/// Parents are set in the same neighbour order as [`GameState::search_path`], so a route read
/// from the field is the route a single search would return.
pub(crate) struct DistanceField {
    reached: HashMap<(u32, u32), (u32, Option<Position>)>,
}

impl DistanceField {
    fn tiles(&self, position: Position) -> Option<u32> {
        self.reached
            .get(&(position.x, position.y))
            .map(|(steps, _)| *steps + 1)
    }

    fn route(&self, goal: Position) -> Option<Vec<Position>> {
        let mut parent = self.reached.get(&(goal.x, goal.y))?.1;
        let mut route = vec![goal];
        while let Some(tile) = parent {
            route.push(tile);
            parent = self.reached.get(&(tile.x, tile.y))?.1;
        }
        route.reverse();
        Some(route)
    }
}

/// A chosen destination.
pub(crate) struct Choice {
    pub habitat_id: u32,
    pub route: Vec<Position>,
    pub plan: String,
}

/// What a ready group does next.
enum Decision {
    Visit(Choice),
    Leave(ExitCause),
}

/// The #81 viewpoint order: free room, relative load, visible animals, walk, then position.
fn compare_spots(left: (&ViewingSpot, u32), right: (&ViewingSpot, u32)) -> Ordering {
    let (left_spot, left_tiles) = left;
    let (right_spot, right_tiles) = right;
    let left_load = u64::from(left_spot.occupancy).saturating_mul(u64::from(right_spot.capacity));
    let right_load = u64::from(right_spot.occupancy).saturating_mul(u64::from(left_spot.capacity));
    right_spot
        .available_capacity()
        .cmp(&left_spot.available_capacity())
        .then_with(|| left_load.cmp(&right_load))
        .then_with(|| right_spot.visible_animals.cmp(&left_spot.visible_animals))
        .then_with(|| left_tiles.cmp(&right_tiles))
        .then_with(|| left_spot.position.y.cmp(&right_spot.position.y))
        .then_with(|| left_spot.position.x.cmp(&right_spot.position.x))
}

impl GameState {
    /// One breadth-first walk for `mover` from `start`, counted as one path search.
    pub(crate) fn distance_field(&self, mover: Mover, start: Position) -> DistanceField {
        if self.walkable(mover, start) {
            self.count_path_search();
        }
        self.walk_distance_field(mover, start)
    }

    /// The same walk, uncounted: presentation queries use it so a snapshot never changes the
    /// simulation work counters.
    pub(crate) fn walk_distance_field(&self, mover: Mover, start: Position) -> DistanceField {
        let mut reached = HashMap::new();
        if !self.walkable(mover, start) {
            return DistanceField { reached };
        }
        reached.insert((start.x, start.y), (0, None));
        let mut queue = VecDeque::from([start]);
        while let Some(current) = queue.pop_front() {
            let steps = reached[&(current.x, current.y)].0;
            for next in self.neighbors(current) {
                if !self.walkable(mover, next) || reached.contains_key(&(next.x, next.y)) {
                    continue;
                }
                reached.insert((next.x, next.y), (steps + 1, Some(current)));
                queue.push_back(next);
            }
        }
        DistanceField { reached }
    }

    /// The best reachable viewpoint of `habitat` by the #81 order, read from `field`.
    pub(crate) fn best_viewpoint(
        &self,
        habitat_id: u32,
        field: &DistanceField,
        occupancy: &ViewingOccupancy,
    ) -> Option<(ViewingSpot, u32)> {
        let habitat = self
            .habitats
            .iter()
            .find(|habitat| habitat.id == habitat_id)?;
        self.viewing_spots_with_occupancy(habitat, occupancy)
            .into_iter()
            .filter(|spot| spot.capacity > 0)
            .filter_map(|spot| field.tiles(spot.position).map(|tiles| (spot, tiles)))
            .min_by(|left, right| compare_spots((&left.0, left.1), (&right.0, right.1)))
    }

    /// The route from the field's start to `habitat`'s best viewpoint.
    pub(crate) fn viewing_route_in(
        &self,
        habitat_id: u32,
        field: &DistanceField,
        occupancy: &ViewingOccupancy,
    ) -> Option<Vec<Position>> {
        let (spot, _) = self.best_viewpoint(habitat_id, field, occupancy)?;
        field.route(spot.position)
    }

    /// The group's next habitat from `start`: the best score among stocked, engaging habitats
    /// not in `seen` whose best viewpoint is within walking tolerance (#103).
    pub(crate) fn choose_attraction(
        &self,
        profile: Profile,
        favorite: Option<&str>,
        seen: &[u32],
        field: &DistanceField,
        occupancy: &ViewingOccupancy,
    ) -> Option<Choice> {
        self.work
            .itinerary_decisions
            .set(self.work.itinerary_decisions.get() + 1);
        let rules = profile.rules();
        let mut best: Option<(i64, u32, ViewingSpot, u32, bool)> = None;
        for habitat in &self.habitats {
            if !habitat.stocked()
                || habitat.welfare() < MIN_ENGAGING_HABITAT_WELFARE
                || seen.contains(&habitat.id)
            {
                continue;
            }
            self.work
                .itinerary_candidates
                .set(self.work.itinerary_candidates.get() + 1);
            let Some((spot, tiles)) = self.best_viewpoint(habitat.id, field, occupancy) else {
                continue;
            };
            if tiles > rules.walking_tolerance {
                continue;
            }
            // A mixed habitat (#108) shows every resident species.
            let favored = favorite.is_some_and(|favorite| {
                habitat
                    .residents
                    .iter()
                    .any(|resident| resident.species.key == favorite)
            });
            let full = spot.available_capacity() == 0;
            let score = i64::from(habitat.appeal().min(300) / 10)
                + i64::from(habitat.welfare() / 5)
                + if favored { rules.favorite_weight } else { 0 }
                - rules.walk_weight * i64::from(tiles)
                - if full { rules.crowd_weight } else { 0 };
            let better = best.as_ref().is_none_or(|(best_score, best_id, ..)| {
                score > *best_score || (score == *best_score && habitat.id < *best_id)
            });
            if better {
                best = Some((score, habitat.id, spot, tiles, favored));
            }
        }
        let (_, habitat_id, spot, tiles, favored) = best?;
        let route = field.route(spot.position)?;
        let species = self
            .habitats
            .iter()
            .find(|habitat| habitat.id == habitat_id)
            .map_or_else(|| "animals".to_owned(), Habitat::species_labels);
        let mut reasons = Vec::new();
        if favored {
            reasons.push("our favourite species".to_owned());
        }
        reasons.push(format!("{tiles} tiles away"));
        reasons.push(if spot.available_capacity() == 0 {
            "every viewpoint is full".to_owned()
        } else {
            "room at the fence".to_owned()
        });
        Some(Choice {
            habitat_id,
            route,
            plan: format!("Habitat #{habitat_id} ({species}): {}", reasons.join(", ")),
        })
    }

    pub(crate) fn group_index(&self, group_id: u32) -> Option<usize> {
        self.groups.iter().position(|group| group.id == group_id)
    }

    fn guest_index(&self, guest_id: u32) -> Option<usize> {
        self.guests.iter().position(|guest| guest.id == guest_id)
    }

    /// The species on show, by key; a new group's favourite is one of them.
    fn exhibited_species(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .habitats
            .iter()
            .filter(|habitat| habitat.stocked())
            .flat_map(Habitat::species_keys)
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Minutes of arrivals the next group at the gate stands for (shorter during a campaign).
    pub(crate) fn next_arrival_minutes(&self) -> u32 {
        group_plan(self.visits.groups_arrived.saturating_add(1)).1
            * self.arrival_minutes_per_guest()
    }

    /// A group comes to the gate when it can reach an attraction and decides there (#106). An
    /// admitted group pays one admission line for all of its people, then each person becomes a
    /// guest record walking from the entrance; a group turned away pays nothing and nobody
    /// enters. Either way it takes its place in the arrival sequence.
    pub(crate) fn try_spawn_group(&mut self) {
        let start = self.entrance;
        let group_id = self.next_group_id;
        let arrival = self.visits.groups_arrived.saturating_add(1);
        let (profile, size) = group_plan(arrival);
        if group_id == u32::MAX || self.next_guest_id.checked_add(size).is_none() {
            return;
        }
        let species = self.exhibited_species();
        let favorite =
            (!species.is_empty()).then(|| species[favorite_index(arrival, species.len())].clone());
        let occupancy = self.viewing_occupancy();
        let field = self.distance_field(Mover::Guest, start);
        let Some(choice) =
            self.choose_attraction(profile, favorite.as_deref(), &[], &field, &occupancy)
        else {
            return;
        };
        // The gate (#106): the group decides once, before anyone enters or pays.
        let (value, _) = self.experience_value(&field, &occupancy);
        let decision = self.gate_decision(arrival, profile, size, value);
        let price = self.pricing.admission_cents;
        if decision.outcome != GateOutcome::Admitted {
            self.visits.groups_arrived = arrival;
            self.record_gate(arrival, profile, size, &decision, 0);
            self.emit("group_turned_away", None);
            return;
        }
        let Some(admission) = price.checked_mul(i64::from(size)) else {
            return;
        };
        // One ledger line for the whole group; a free park records none.
        if admission > 0 && !self.earn(admission, super::IncomeCategory::Admissions) {
            return;
        }
        self.record_gate(arrival, profile, size, &decision, admission);
        let arrival_value = pricing::arrival_value(price, value);
        self.next_group_id += 1;
        let mut planned = occupancy;
        let mut members = Vec::new();
        for budget in &decision.budgets {
            let id = self.next_guest_id;
            let route = self
                .viewing_route_in(choice.habitat_id, &field, &planned)
                .unwrap_or_else(|| choice.route.clone());
            if let Some(end) = route.last() {
                *planned.entry((end.x, end.y)).or_default() += 1;
            }
            self.guests.push(Guest::arriving(
                Arrival {
                    id,
                    group_id,
                    start,
                    // What is left of the budget after admission is the spending money.
                    cash_cents: budget - price,
                    value_perception: arrival_value,
                },
                choice.habitat_id,
                route,
            ));
            self.emit("guest_arrived", Some(id));
            members.push(id);
            self.next_guest_id += 1;
        }
        self.groups.push(Group {
            id: group_id,
            profile,
            favorite_species: favorite,
            admitted: size,
            members,
            waiting_minutes: 0,
            plan: choice.plan,
        });
        self.visits.groups_arrived = arrival;
        self.emit("group_arrived", Some(group_id));
    }

    /// A guest's own reason to stop, if any: needs first, then crowding, then value.
    pub(crate) fn blocking_cause(&self, guest: &Guest, profile: Profile) -> Option<ExitCause> {
        // A pressing need ends the visit only once the park failed to meet it; otherwise the
        // guest sees to it on the way to the next habitat (#101, #102).
        let pressing = guest.unmet_need.and_then(|need| {
            let (pressing, what) = match need {
                comfort::Need::Toilet => (
                    guest.bladder >= comfort::BLADDER_DESPERATE,
                    "needs a toilet",
                ),
                comfort::Need::Drink => (guest.thirst > GUEST_CONTINUE_MAX_THIRST, "is thirsty"),
                comfort::Need::Food => (guest.hunger > GUEST_CONTINUE_MAX_HUNGER, "is hungry"),
                comfort::Need::Rest => (guest.energy < GUEST_CONTINUE_MIN_ENERGY, "is worn out"),
            };
            pressing.then_some(what)
        });
        if let Some(what) = pressing {
            return Some(ExitCause {
                reason: ExitReason::NeedsUnmet,
                detail: format!("Guest #{} {what} and the park could not help.", guest.id),
            });
        }
        let patience = profile.rules().patience;
        if guest.crowd_minutes >= patience {
            return Some(ExitCause {
                reason: ExitReason::TooCrowded,
                detail: format!(
                    "Guest #{} spent {} minutes in crowds (patience {patience}).",
                    guest.id, guest.crowd_minutes
                ),
            });
        }
        if guest.value_perception < GUEST_CONTINUE_MIN_VALUE {
            return Some(ExitCause {
                reason: ExitReason::PoorValue,
                detail: format!(
                    "Guest #{} rated the visit's value {}.",
                    guest.id, guest.value_perception
                ),
            });
        }
        if guest.happiness < GUEST_CONTINUE_MIN_HAPPINESS {
            let (reason, why) = if guest.crowd_minutes * 2 >= patience {
                (ExitReason::TooCrowded, "crowds")
            } else if guest.unmet_need.is_some() {
                (ExitReason::NeedsUnmet, "an unmet need")
            } else {
                (ExitReason::PoorValue, "a disappointing visit")
            };
            return Some(ExitCause {
                reason,
                detail: format!(
                    "Guest #{} was unhappy ({}) after {why}.",
                    guest.id, guest.happiness
                ),
            });
        }
        None
    }

    /// Habitats any member of the group has watched.
    fn group_seen(&self, members: &[usize]) -> Vec<u32> {
        let mut seen: Vec<u32> = members
            .iter()
            .flat_map(|index| self.guests[*index].visited_habitats.iter().copied())
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    }

    /// The group's decision; `field` is the walk from its first ready member's tile.
    fn decide(
        &self,
        group: &Group,
        members: &[usize],
        field: &DistanceField,
        occupancy: &ViewingOccupancy,
    ) -> Decision {
        for index in members {
            if let Some(cause) = self.blocking_cause(&self.guests[*index], group.profile) {
                return Decision::Leave(cause);
            }
        }
        let seen = self.group_seen(members);
        let goal = group.profile.rules().visit_goal;
        if seen.len() as u32 >= goal {
            return Decision::Leave(ExitCause {
                reason: ExitReason::Satisfied,
                detail: format!("Saw {} habitats, as hoped.", seen.len()),
            });
        }
        match self.choose_attraction(
            group.profile,
            group.favorite_species.as_deref(),
            &seen,
            field,
            occupancy,
        ) {
            Some(choice) => Decision::Visit(choice),
            None => Decision::Leave(ExitCause {
                reason: ExitReason::NoReachableAttraction,
                detail: format!(
                    "Saw {} of the {goal} habitats hoped for; no other engaging habitat is within {} tiles.",
                    seen.len(),
                    group.profile.rules().walking_tolerance
                ),
            }),
        }
    }

    /// Moves a guest out of its group into a new group of its own with the same profile.
    fn split_off(&mut self, guest_id: u32) {
        let Some(index) = self.guest_index(guest_id) else {
            return;
        };
        let Some(group) = self.group_index(self.guests[index].group_id) else {
            return;
        };
        if self.groups[group].members.len() <= 1 {
            return;
        }
        self.groups[group].members.retain(|id| *id != guest_id);
        let id = self.next_group_id;
        self.next_group_id = self.next_group_id.saturating_add(1);
        let source = &self.groups[group];
        let solo = Group {
            id,
            profile: source.profile,
            favorite_species: source.favorite_species.clone(),
            admitted: 0,
            members: vec![guest_id],
            waiting_minutes: 0,
            plan: source.plan.clone(),
        };
        self.groups.push(solo);
        self.guests[index].group_id = id;
        self.emit("group_split", Some(id));
    }

    /// Whether another member of the guest's group is already waiting for the rest.
    pub(crate) fn group_waiting(&self, index: usize) -> bool {
        let guest = &self.guests[index];
        self.group_index(guest.group_id).is_some_and(|group| {
            self.groups[group].members.iter().any(|id| {
                *id != guest.id
                    && self
                        .guest_index(*id)
                        .is_some_and(|other| self.guests[other].ready())
            })
        })
    }

    /// Sends a guest towards the gate for `cause`; a guest with no route waits where it is.
    fn send_to_exit(&mut self, index: usize, cause: ExitCause) -> bool {
        let start = self.guests[index].position();
        let Some(route) = self.path_between(Mover::Guest, start, self.entrance) else {
            return false;
        };
        let guest = &mut self.guests[index];
        guest.state = GuestState::WalkingToExit;
        guest.route = route;
        guest.route_index = 0;
        guest.exit = Some(cause);
        true
    }

    /// Every minute: ready groups decide together; a group whose stragglers kept it waiting too
    /// long lets them go first.
    pub(crate) fn advance_groups(&mut self) {
        let occupancy = self.viewing_occupancy();
        let mut planned = occupancy;
        let mut group_index = 0;
        while group_index < self.groups.len() {
            let members: Vec<usize> = self.groups[group_index]
                .members
                .iter()
                .filter_map(|id| self.guest_index(*id))
                .collect();
            let ready: Vec<usize> = members
                .iter()
                .copied()
                .filter(|index| self.guests[*index].ready())
                .collect();
            if ready.is_empty() {
                self.groups[group_index].waiting_minutes = 0;
                group_index += 1;
                continue;
            }
            if ready.len() < members.len() {
                let group = &mut self.groups[group_index];
                group.waiting_minutes = group.waiting_minutes.saturating_add(1);
                if group.waiting_minutes < GROUP_WAIT_MINUTES {
                    group_index += 1;
                    continue;
                }
                let stragglers: Vec<u32> = members
                    .iter()
                    .filter(|index| !ready.contains(index))
                    .map(|index| self.guests[*index].id)
                    .collect();
                for guest_id in stragglers {
                    self.split_off(guest_id);
                }
            }
            self.groups[group_index].waiting_minutes = 0;
            let leader = self.guests[ready[0]].position();
            let mut fields = vec![(leader, self.distance_field(Mover::Guest, leader))];
            let decision = self.decide(&self.groups[group_index], &ready, &fields[0].1, &planned);
            let ready_ids: Vec<u32> = ready.iter().map(|index| self.guests[*index].id).collect();
            match decision {
                Decision::Visit(choice) => {
                    self.groups[group_index].plan = choice.plan;
                    for guest_id in ready_ids {
                        let Some(index) = self.guest_index(guest_id) else {
                            continue;
                        };
                        let start = self.guests[index].position();
                        if !fields.iter().any(|(at, _)| *at == start) {
                            let field = self.distance_field(Mover::Guest, start);
                            fields.push((start, field));
                        }
                        // Members pick viewpoints in turn, so a group spreads over free room.
                        let route =
                            fields
                                .iter()
                                .find(|(at, _)| *at == start)
                                .and_then(|(_, field)| {
                                    self.viewing_route_in(choice.habitat_id, field, &planned)
                                });
                        let Some(route) = route else {
                            // Cut off from the group's choice: carry on alone.
                            self.split_off(guest_id);
                            continue;
                        };
                        if let Some(end) = route.last() {
                            *planned.entry((end.x, end.y)).or_default() += 1;
                        }
                        let guest = &mut self.guests[index];
                        guest.target_habitat = choice.habitat_id;
                        guest.state = GuestState::WalkingToHabitat;
                        guest.route = route;
                        guest.route_index = 0;
                    }
                }
                Decision::Leave(cause) => {
                    for guest_id in ready_ids {
                        let Some(index) = self.guest_index(guest_id) else {
                            continue;
                        };
                        let own = self
                            .blocking_cause(&self.guests[index], self.groups[group_index].profile);
                        if !self.send_to_exit(index, own.unwrap_or_else(|| cause.clone())) {
                            // No way out from here: wait alone and retry.
                            self.split_off(guest_id);
                        }
                    }
                }
            }
            group_index += 1;
        }
    }

    /// A guest back from an errand: rejoin the group's habitat, or decide alone.
    pub(crate) fn resume_itinerary(&mut self, index: usize) {
        let guest = &self.guests[index];
        let target = guest.target_habitat;
        let rejoin = self
            .habitats
            .iter()
            .any(|habitat| habitat.id == target && habitat.stocked())
            && (!guest.visited_habitats.contains(&target)
                || self
                    .group_index(guest.group_id)
                    .is_some_and(|group| self.groups[group].members.len() > 1));
        if rejoin {
            let start = guest.position();
            let occupancy = self.viewing_occupancy();
            let field = self.distance_field(Mover::Guest, start);
            if let Some(route) = self.viewing_route_in(target, &field, &occupancy) {
                let guest = &mut self.guests[index];
                guest.state = GuestState::WalkingToHabitat;
                guest.route = route;
                guest.route_index = 0;
                return;
            }
        }
        // Alone, or the group's habitat is out of reach: carry on alone from here.
        let guest_id = self.guests[index].id;
        self.split_off(guest_id);
        let Some(group) = self.group_index(self.guests[index].group_id) else {
            return;
        };
        let occupancy = self.viewing_occupancy();
        let field = self.distance_field(Mover::Guest, self.guests[index].position());
        match self.decide(&self.groups[group], &[index], &field, &occupancy) {
            Decision::Visit(choice) => {
                self.groups[group].plan = choice.plan;
                let guest = &mut self.guests[index];
                guest.target_habitat = choice.habitat_id;
                guest.state = GuestState::WalkingToHabitat;
                guest.route = choice.route;
                guest.route_index = 0;
            }
            Decision::Leave(cause) => {
                if !self.send_to_exit(index, cause.clone()) {
                    // Cut off from the gate: wait in place until a path edit re-plans the walk.
                    let guest = &mut self.guests[index];
                    guest.state = GuestState::WalkingToExit;
                    guest.route.clear();
                    guest.route_index = 0;
                    guest.exit = Some(cause);
                }
            }
        }
    }

    /// Guests who walked out of the gate: count each person's reason and leave their group.
    pub(crate) fn record_exits(&mut self, leaving: &[u32]) {
        let minute = self.absolute_minute();
        for guest_id in leaving {
            let Some(index) = self.guest_index(*guest_id) else {
                continue;
            };
            let guest = &self.guests[index];
            let cause = guest.exit.clone().unwrap_or_else(|| ExitCause {
                reason: ExitReason::Satisfied,
                detail: format!("Saw {} habitats.", guest.visited_habitats.len()),
            });
            self.visits.today.add(cause.reason);
            self.visits.total.add(cause.reason);
            self.visits.recent.push(ExitNote {
                guest_id: *guest_id,
                group_id: guest.group_id,
                minute,
                reason: cause.reason,
                detail: cause.detail,
            });
        }
        let excess = self.visits.recent.len().saturating_sub(RECENT_EXITS);
        self.visits.recent.drain(..excess);
        self.forget_guests(leaving);
    }

    /// Removes departed guests from their groups and drops groups left empty.
    pub(crate) fn forget_guests(&mut self, guest_ids: &[u32]) {
        for group in &mut self.groups {
            group.members.retain(|id| !guest_ids.contains(id));
        }
        self.groups.retain(|group| !group.members.is_empty());
    }

    /// One minute of crowding: path tiles shared by too many guests, and over-full viewpoints.
    pub(crate) fn advance_crowding(&mut self) {
        let counts = self.tile_guest_counts();
        let occupancy = self.viewing_occupancy();
        let crowded_views: Vec<bool> = self
            .guests
            .iter()
            .map(|guest| {
                guest.state == GuestState::Viewing
                    && self
                        .viewing_spot_for_guest_with_occupancy(guest, &occupancy)
                        .is_some_and(ViewingSpot::crowded)
            })
            .collect();
        for (guest, crowded_view) in self.guests.iter_mut().zip(crowded_views) {
            let crowded_path = counts
                .get(&(guest.x, guest.y))
                .is_some_and(|count| *count > PATH_COMFORT_GUESTS);
            if crowded_path || crowded_view {
                guest.crowd_minutes = (guest.crowd_minutes + 1).min(MAX_CROWD_MINUTES);
                if crowded_path && guest.crowd_minutes % 5 == 0 {
                    guest.happiness = guest.happiness.saturating_sub(1);
                }
            }
        }
    }

    pub(crate) fn tile_guest_counts(&self) -> HashMap<(u32, u32), u32> {
        let mut counts = HashMap::new();
        for guest in &self.guests {
            *counts.entry((guest.x, guest.y)).or_insert(0u32) += 1;
        }
        counts
    }

    /// Whether the guest's tile is crowded now.
    #[cfg(test)]
    pub(crate) fn guest_on_crowded_tile(&self, guest: &Guest) -> bool {
        self.guests
            .iter()
            .filter(|other| other.x == guest.x && other.y == guest.y)
            .count() as u32
            > PATH_COMFORT_GUESTS
    }

    /// The group view of one guest.
    pub(crate) fn guest_group_view(&self, guest: &Guest) -> Option<GuestGroupView> {
        let group = &self.groups[self.group_index(guest.group_id)?];
        let rules = group.profile.rules();
        let waiting = guest.ready() && group.members.len() > 1;
        Some(GuestGroupView {
            id: group.id,
            profile: group.profile,
            profile_label: group.profile.label(),
            favorite_species: group.favorite_species.clone(),
            members: group.members.clone(),
            admitted: group.admitted,
            visit_goal: rules.visit_goal,
            walking_tolerance: rules.walking_tolerance,
            patience: rules.patience,
            plan: group.plan.clone(),
            waiting_for_group: waiting,
            waiting_minutes: group.waiting_minutes,
        })
    }

    pub(crate) fn visits_view(&self) -> VisitsView {
        VisitsView {
            guests_in_park: self.guests.len() as u32,
            groups_in_park: self.groups.len() as u32,
            groups_arrived: self.visits.groups_arrived,
            guests_arrived: self.next_guest_id.saturating_sub(1),
            today: self.visits.today,
            total: self.visits.total,
            recent: self.visits.recent.clone(),
            crowded_tiles: {
                let mut tiles: Vec<CrowdedTile> = self
                    .tile_guest_counts()
                    .into_iter()
                    .filter(|(_, guests)| *guests > PATH_COMFORT_GUESTS)
                    .map(|((x, y), guests)| CrowdedTile { x, y, guests })
                    .collect();
                tiles.sort_by_key(|tile| (tile.y, tile.x));
                tiles
            },
        }
    }

    pub(crate) fn visit_violation(&self) -> Option<String> {
        let mut seen = std::collections::HashSet::new();
        for group in &self.groups {
            if group.id >= self.next_group_id {
                return Some(format!("group {} is not below the next group ID", group.id));
            }
            if group.members.is_empty() {
                return Some(format!("group {} has no members", group.id));
            }
            for member in &group.members {
                if !seen.insert(*member) {
                    return Some(format!("guest {member} is in two groups"));
                }
                match self.guests.iter().find(|guest| guest.id == *member) {
                    Some(guest) if guest.group_id == group.id => {}
                    _ => return Some(format!("group {} lists guest {member} wrongly", group.id)),
                }
            }
        }
        if let Some(guest) = self.guests.iter().find(|guest| !seen.contains(&guest.id)) {
            return Some(format!("guest {} belongs to no group", guest.id));
        }
        None
    }
}

/// Who walks in through the gate: identity, start tile and what the gate left them (#106).
pub(crate) struct Arrival {
    pub id: u32,
    pub group_id: u32,
    pub start: Position,
    pub cash_cents: i64,
    pub value_perception: u32,
}

impl Guest {
    fn arriving(arrival: Arrival, habitat: u32, route: Vec<Position>) -> Self {
        let Arrival {
            id,
            group_id,
            start,
            cash_cents,
            value_perception,
        } = arrival;
        Self {
            id,
            x: start.x,
            y: start.y,
            happiness: 78,
            energy: 90,
            hunger: 18,
            thirst: 16,
            value_perception,
            minutes_in_park: 0,
            target_habitat: habitat,
            state: GuestState::Arriving,
            route,
            route_index: 0,
            viewing_minutes: 0,
            arrival_steps: 2,
            bought_food: false,
            bought_drink: false,
            visited_habitats: Vec::new(),
            cleanliness_concern: false,
            bladder: comfort::ARRIVAL_BLADDER,
            visit: None,
            unmet_need: None,
            next_need_check: 0,
            cash_cents,
            group_id,
            crowd_minutes: 0,
            exit: None,
        }
    }

    pub(crate) fn position(&self) -> Position {
        Position {
            x: self.x,
            y: self.y,
        }
    }

    /// Finished watching and waiting for the group's next decision.
    pub(crate) fn ready(&self) -> bool {
        self.state == GuestState::Viewing && self.viewing_minutes == 0
    }
}

#[derive(Serialize)]
pub(crate) struct GuestGroupView {
    id: u32,
    profile: Profile,
    profile_label: &'static str,
    favorite_species: Option<String>,
    members: Vec<u32>,
    admitted: u32,
    visit_goal: u32,
    walking_tolerance: u32,
    patience: u32,
    plan: String,
    waiting_for_group: bool,
    waiting_minutes: u32,
}

#[derive(Serialize)]
struct CrowdedTile {
    x: u32,
    y: u32,
    guests: u32,
}

#[derive(Serialize)]
pub(crate) struct VisitsView {
    guests_in_park: u32,
    groups_in_park: u32,
    groups_arrived: u32,
    guests_arrived: u32,
    today: ExitCounts,
    total: ExitCounts,
    recent: Vec<ExitNote>,
    crowded_tiles: Vec<CrowdedTile>,
}
