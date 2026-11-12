// Visitor profiles, groups, itineraries, crowding and exit reasons (#103).
//
// The authored park: the default 20x14 meadow with the entrance at (0, 7) and a public path
// along y = 7 to x = 18. A capybara habitat lies just south of the entrance path, a zebra
// habitat further along and a giraffe habitat north of the path's far end; toilets and stands
// sit on the north side of the path between them.
use super::*;

const CAPYBARA: (u32, u32, u32, u32) = (3, 8, 5, 10);
const ZEBRA: (u32, u32, u32, u32) = (9, 8, 11, 10);
const GIRAFFE: (u32, u32, u32, u32) = (15, 4, 17, 6);

fn habitat(state: &mut GameState, rect: (u32, u32, u32, u32), species: &str) -> u32 {
    assert!(state.place_habitat_rect(rect.0, rect.1, rect.2, rect.3).ok);
    let id = state.habitats.last().unwrap().id;
    staff_habitat(state, id);
    if species == "giraffe" {
        assert!(state.upgrade_fence(id, "steel_mesh").ok);
    }
    assert!(state.adopt(id, species).ok, "{species}");
    id
}

/// The authored park, with or without its toilet, drink and food stands.
fn itinerary_park(services: bool) -> (GameState, [u32; 3]) {
    let mut state = GameState::default();
    let result = state.build_path_stroke(&[1, 7, 18, 7], 1, "public");
    assert!(result.ok, "{}", result.message);
    let capybara = habitat(&mut state, CAPYBARA, "capybara");
    let zebra = habitat(&mut state, ZEBRA, "zebra");
    let giraffe = habitat(&mut state, GIRAFFE, "giraffe");
    if services {
        for (x, kind) in [(7, "toilet"), (8, "drink"), (12, "food"), (13, "toilet")] {
            let result = state.place_concession(x, 6, kind);
            assert!(result.ok, "{kind}: {}", result.message);
        }
        assert!(state.hire_mechanic().ok);
        assert!(state.hire_janitor().ok);
    }
    (state, [capybara, zebra, giraffe])
}

/// The first group ID after `from` whose plan and favourite (among `exhibited`) satisfy `want`.
fn arrival_where(
    from: u32,
    exhibited: &[&str],
    want: impl Fn(visit::Profile, u32, &str) -> bool,
) -> u32 {
    (from..)
        .find(|arrival| {
            let (profile, size) = visit::group_plan(*arrival);
            let favorite = exhibited[visit::favorite_index(*arrival, exhibited.len())];
            want(profile, size, favorite)
        })
        .unwrap()
}

/// Lets the `arrival`-th group in at the entrance now; returns its guest IDs.
fn spawn(state: &mut GameState, arrival: u32) -> Vec<u32> {
    state.visits.groups_arrived = arrival - 1;
    let before = state.guests.len();
    state.try_spawn_group();
    assert_eq!(state.visits.groups_arrived, arrival, "the group arrived");
    state.guests[before..].iter().map(|guest| guest.id).collect()
}

fn group_of(state: &GameState, guest_id: u32) -> u32 {
    state.guests.iter().find(|g| g.id == guest_id).unwrap().group_id
}

const EXHIBITED: [&str; 3] = ["capybara", "giraffe", "zebra"];

/// No guest ever moves more than one tile a minute or stands off a guest-walkable tile.
fn assert_no_teleport(before: &GameState, after: &GameState) {
    for guest in &after.guests {
        assert!(
            after.walkable(Mover::Guest, guest.position()),
            "guest {} off the path at {:?}",
            guest.id,
            guest.position()
        );
        match before.guests.iter().find(|old| old.id == guest.id) {
            Some(old) => assert!(
                old.x.abs_diff(guest.x) + old.y.abs_diff(guest.y) <= 1,
                "guest {} jumped from {:?} to {:?}",
                guest.id,
                old.position(),
                guest.position()
            ),
            None => assert_eq!(guest.position(), after.entrance, "guest {} appeared", guest.id),
        }
    }
}

/// Runs `minutes` with no new arrivals, checking movement every minute, until the tracked
/// guests have left; returns the habitats each watched (in order) and its exit cause.
fn follow(
    state: &mut GameState,
    tracked: &[u32],
    minutes: u32,
) -> HashMap<u32, (Vec<u32>, Option<visit::ExitCause>)> {
    let mut seen: HashMap<u32, (Vec<u32>, Option<visit::ExitCause>)> = HashMap::new();
    for _ in 0..minutes {
        state.spawn_accumulator = 0;
        let before = state.clone();
        state.tick(1);
        assert_no_teleport(&before, state);
        assert_eq!(state.visit_violation(), None);
        for guest in state.guests.iter().filter(|guest| tracked.contains(&guest.id)) {
            seen.insert(guest.id, (guest.visited_habitats.clone(), guest.exit.clone()));
        }
        if tracked
            .iter()
            .all(|id| !state.guests.iter().any(|guest| guest.id == *id))
        {
            break;
        }
    }
    seen
}

#[test]
fn two_profiles_choose_meaningfully_different_valid_itineraries() {
    let (mut state, [capybara, zebra, giraffe]) = itinerary_park(true);
    let family = arrival_where(1, &EXHIBITED, |profile, _, favorite| {
        profile == visit::Profile::Family && favorite == "capybara"
    });
    let enthusiast = arrival_where(1, &EXHIBITED, |profile, size, favorite| {
        profile == visit::Profile::Enthusiast && size == 1 && favorite == "giraffe"
    });
    let family_guests = spawn(&mut state, family);
    let enthusiast_guests = spawn(&mut state, enthusiast);
    let mut tracked = family_guests.clone();
    tracked.extend(&enthusiast_guests);
    // Only the two staged groups visit.
    let seen = follow(&mut state, &tracked, 600);
    assert!(state.guests.is_empty(), "both groups left");
    let quiet = state;

    // The family sees the two nearest habitats and leaves satisfied.
    for id in &family_guests {
        let (visited, exit) = &seen[id];
        assert_eq!(visited, &vec![capybara, zebra], "family guest {id}");
        assert_eq!(exit.as_ref().unwrap().reason, visit::ExitReason::Satisfied);
    }
    // The enthusiast walks to its favourite giraffes first, then takes in the rest.
    let (visited, exit) = &seen[&enthusiast_guests[0]];
    assert_eq!(visited, &vec![giraffe, zebra, capybara]);
    // It hoped for four habitats; the park has three.
    assert_eq!(
        exit.as_ref().unwrap().reason,
        visit::ExitReason::NoReachableAttraction
    );
    assert_eq!(quiet.visits.total.satisfied, family_guests.len() as u32);
    assert_eq!(quiet.visits.total.no_reachable_attraction, 1);
}

/// Every itinerary choice reads one breadth-first walk; the route it picks is the one the #81
/// viewpoint search would pick for the same start (PRINCIPLE-010 reference).
#[test]
fn itinerary_routes_match_the_viewpoint_search_and_cost_one_walk() {
    let (state, ids) = itinerary_park(true);
    let occupancy = state.viewing_occupancy();
    for start in [at(0, 7), at(6, 7), at(14, 7), at(18, 7)] {
        let field = state.distance_field(Mover::Guest, start);
        for id in ids {
            let habitat = state.habitats.iter().find(|h| h.id == id).unwrap();
            assert_eq!(
                state.viewing_route_in(id, &field, &occupancy),
                state.viewing_route_with_occupancy(habitat, start, &occupancy),
                "habitat {id} from {start:?}"
            );
        }
    }
    let searches = state.work.path_searches.get();
    let field = state.distance_field(Mover::Guest, state.entrance);
    let choice = state.choose_attraction(visit::Profile::Stroller, None, &[], &field, &occupancy);
    assert!(choice.is_some());
    assert_eq!(state.work.path_searches.get(), searches + 1);
    assert_eq!(state.work.itinerary_decisions.get(), 1);
    assert_eq!(state.work.itinerary_candidates.get(), 3);
}

fn at(x: u32, y: u32) -> Position {
    Position { x, y }
}

/// Runs a park for a day with no other change and returns its visit record and admissions.
fn day_of_visits(mut state: GameState) -> (visit::VisitRecord, i64, u32) {
    for _ in 0..720 {
        state.tick(1);
    }
    (
        state.visits.clone(),
        state.finance_today.admissions_income_cents,
        state.next_guest_id,
    )
}

#[test]
fn better_service_placement_measurably_improves_visits_without_changing_arrivals() {
    let (without, _) = itinerary_park(false);
    let (with, _) = itinerary_park(true);
    let (poor, poor_admissions, poor_arrivals) = day_of_visits(without);
    let (good, good_admissions, good_arrivals) = day_of_visits(with);
    // Arrivals and admission are unrelated to the services: the same people came and paid.
    assert_eq!(poor.groups_arrived, good.groups_arrived);
    assert_eq!(poor_arrivals, good_arrivals);
    assert_eq!(poor_admissions, good_admissions);
    // Without toilets and stands, needs end visits; with them, more guests leave satisfied.
    assert!(poor.total.needs_unmet > good.total.needs_unmet, "{poor:?} vs {good:?}");
    assert!(good.total.satisfied > poor.total.satisfied, "{poor:?} vs {good:?}");
    assert!(
        poor.recent
            .iter()
            .any(|note| note.reason == visit::ExitReason::NeedsUnmet
                && note.detail.contains("needs a toilet")),
        "{:?}",
        poor.recent
    );
}

#[test]
fn better_viewing_frontage_measurably_reduces_crowding() {
    // One habitat seen only from a short stretch of path: groups crowd its few viewpoints.
    let narrow = || {
        let mut state = GameState::default();
        assert!(state.build_path_stroke(&[1, 7, 6, 7], 1, "public").ok);
        habitat(&mut state, (3, 8, 5, 10), "capybara");
        for _ in 0..2 {
            assert!(state.adopt(1, "capybara").ok);
        }
        for (x, kind) in [(5, "toilet"), (6, "drink")] {
            assert!(state.place_concession(x, 6, kind).ok);
        }
        state
    };
    // A busy hour: five families of four come in at once; returns the crowded minutes they
    // spent in total, and their exit reasons.
    let busy_hour = |mut state: GameState| {
        let mut tracked = Vec::new();
        let mut arrival = 0;
        for _ in 0..5 {
            arrival = visit::arrival_of_size(4, arrival + 1);
            tracked.extend(spawn(&mut state, arrival));
        }
        let mut crowded: HashMap<u32, u32> = HashMap::new();
        for _ in 0..400 {
            state.spawn_accumulator = 0;
            state.tick(1);
            for guest in state.guests.iter().filter(|g| tracked.contains(&g.id)) {
                crowded.insert(guest.id, guest.crowd_minutes);
            }
        }
        assert!(state.guests.is_empty(), "everyone left");
        (crowded.values().sum::<u32>(), state.visits.total)
    };
    let tight = narrow();
    let mut open = narrow();
    // A wider frontage: path round the west, south and east fences too.
    assert!(open.build_path_stroke(&[2, 7, 2, 11, 6, 11, 6, 7], 1, "public").ok);
    let (tight_crowd, tight_exits) = busy_hour(tight);
    let (open_crowd, open_exits) = busy_hour(open);
    assert!(tight_crowd > 0);
    assert!(open_crowd < tight_crowd, "{open_crowd} vs {tight_crowd}");
    assert!(
        open_exits.too_crowded <= tight_exits.too_crowded,
        "{open_exits:?} vs {tight_exits:?}"
    );
}

#[test]
fn crowded_tiles_are_a_bounded_rule_that_keeps_the_viewing_capacity() {
    let (mut state, [capybara, ..]) = itinerary_park(false);
    let family = spawn(&mut state, visit::arrival_of_size(4, 1));
    let next = visit::arrival_of_size(4, state.visits.groups_arrived + 1);
    let more = spawn(&mut state, next);
    // Eight people on one tile of path, past the six it comfortably holds.
    for guest in &mut state.guests {
        guest.x = 8;
        guest.y = 7;
        guest.route = vec![at(8, 7), at(7, 7)];
        guest.route_index = 0;
        guest.state = GuestState::WalkingToHabitat;
    }
    state.advance_crowding();
    assert!(state.guests.iter().all(|guest| guest.crowd_minutes == 1));
    assert_eq!(state.guest_thought(&state.guests[0]), "It's so crowded here.");
    let overlay = serde_json::to_string(&state.overlay(overlay::OverlayKind::Viewing)).unwrap();
    assert!(overlay.contains("Crowded path 8,7"), "{overlay}");
    assert_eq!(family.len() + more.len(), 8);
    // Six on a tile is fine.
    let gone = &more[2..];
    state.guests.retain(|guest| !gone.contains(&guest.id));
    state.forget_guests(gone);
    assert_eq!(state.visit_violation(), None);
    assert_eq!(state.guests.len(), 6);
    state.advance_crowding();
    assert!(state.guests.iter().all(|guest| guest.crowd_minutes == 1));
    // Crowding is counted only up to its bound.
    for guest in &mut state.guests {
        guest.crowd_minutes = 600;
    }
    state.guests.push(state.guests[0].clone());
    state.guests.last_mut().unwrap().id = state.next_guest_id;
    state.advance_crowding();
    assert!(state.guests.iter().all(|guest| guest.crowd_minutes <= 600));
    // #81's per-tile viewing capacity is unchanged: min(4, visible animals * 2).
    let habitat = state.habitats.iter().find(|h| h.id == capybara).unwrap();
    for spot in state.viewing_spots_with_occupancy(habitat, &ViewingOccupancy::new()) {
        assert_eq!(spot.capacity, (spot.visible_animals * 2).min(4));
    }
}

#[test]
fn full_viewpoints_send_groups_elsewhere_or_on_without_deadlock() {
    let (mut state, [capybara, ..]) = itinerary_park(true);
    // Every capybara viewpoint is over capacity with lone watchers who never finish.
    let habitat = state.habitats.iter().find(|h| h.id == capybara).unwrap().clone();
    let spots = state.viewing_spots_with_occupancy(&habitat, &ViewingOccupancy::new());
    let template = {
        let id = spawn(&mut state, visit::arrival_of_size(1, 1))[0];
        state.guests.iter().find(|guest| guest.id == id).unwrap().clone()
    };
    state.guests.clear();
    state.groups.clear();
    for spot in spots.iter().filter(|spot| spot.capacity > 0) {
        for _ in 0..=spot.capacity {
            let id = state.next_guest_id;
            state.next_guest_id += 1;
            let group = state.next_group_id;
            state.next_group_id += 1;
            state.guests.push(Guest {
                id,
                group_id: group,
                x: spot.position.x,
                y: spot.position.y,
                state: GuestState::Viewing,
                viewing_minutes: 10_000,
                target_habitat: capybara,
                visited_habitats: vec![capybara],
                route: Vec::new(),
                ..template.clone()
            });
            state.groups.push(visit::Group {
                id: group,
                profile: visit::Profile::Enthusiast,
                favorite_species: None,
                admitted: 1,
                members: vec![id],
                waiting_minutes: 0,
                plan: String::new(),
            });
        }
    }
    assert_eq!(state.visit_violation(), None);
    let family_arrival = arrival_where(1, &EXHIBITED, |p, _, _| p == visit::Profile::Family);
    let family = spawn(&mut state, family_arrival);
    // The family avoids the full capybara fence for the zebras, then heads on.
    let target = state.guests.iter().find(|g| g.id == family[0]).unwrap().target_habitat;
    assert_ne!(target, capybara);
    let seen = follow(&mut state, &family, 400);
    for id in &family {
        assert!(!state.guests.iter().any(|guest| guest.id == *id), "guest {id} left");
        assert!(seen[id].1.is_some());
    }
}

#[test]
fn a_straggler_leaves_its_group_after_a_bounded_wait_without_moving() {
    let (mut state, [capybara, ..]) = itinerary_park(true);
    let arrival = arrival_where(1, &EXHIBITED, |profile, size, favorite| {
        profile == visit::Profile::Family && size == 4 && favorite == "capybara"
    });
    let members = spawn(&mut state, arrival);
    let id = group_of(&state, members[0]);
    // Run until the family watches the capybaras.
    for _ in 0..120 {
        state.tick(1);
        if state
            .guests
            .iter()
            .filter(|g| members.contains(&g.id))
            .all(|g| g.state == GuestState::Viewing)
        {
            break;
        }
    }
    // One member is cut off on the way (an empty route waits in place).
    let straggler = members[3];
    let index = state.guests.iter().position(|g| g.id == straggler).unwrap();
    state.guests[index].state = GuestState::WalkingToHabitat;
    state.guests[index].route.clear();
    // Content for now, so no errand takes it anywhere.
    state.guests[index].bladder = 0;
    state.guests[index].hunger = 0;
    state.guests[index].thirst = 0;
    state.guests[index].energy = 100;
    let stuck_at = state.guests[index].position();
    let mut waited = 0;
    loop {
        let before = state.clone();
        state.spawn_accumulator = 0;
        state.tick(1);
        assert_no_teleport(&before, &state);
        assert_eq!(state.visit_violation(), None);
        waited += 1;
        let group = state.group_index(id).map(|g| state.groups[g].members.clone());
        if group.as_ref().is_some_and(|m| !m.contains(&straggler)) {
            break;
        }
        assert!(waited <= 24 + visit::GROUP_WAIT_MINUTES + 10 + 34, "the group waited forever");
    }
    // The others moved on together; the straggler stays where it was, alone, admitted with them.
    let straggler_guest = state.guests.iter().find(|g| g.id == straggler).unwrap();
    assert_eq!(straggler_guest.position(), stuck_at);
    let solo = &state.groups[state.group_index(straggler_guest.group_id).unwrap()];
    assert_eq!(solo.members, vec![straggler]);
    assert_eq!(solo.admitted, 0);
    let family = &state.groups[state.group_index(id).unwrap()];
    assert_eq!(family.members, members[..3].to_vec());
    assert_eq!(family.admitted, 4);
    for guest in state.guests.iter().filter(|g| members[..3].contains(&g.id)) {
        assert_ne!(guest.target_habitat, capybara, "{guest:?}");
    }
    // A path edit gives the straggler its route back; it carries on alone.
    state.topology_changed();
    let seen = follow(&mut state, &[straggler], 300);
    assert!(seen[&straggler].1.is_some());
}

#[test]
fn path_edits_and_disconnected_services_never_deadlock_or_teleport_guests() {
    let (mut state, _) = itinerary_park(true);
    // A toilet on an isolated stub of path nobody can reach.
    assert!(state.build_path_stroke(&[18, 2, 18, 2], 1, "public").ok);
    assert!(state.place_concession(18, 1, "toilet").ok);
    let mut cut = false;
    for minute in 0..900 {
        let before = state.clone();
        state.tick(1);
        assert_no_teleport(&before, &state);
        assert_eq!(state.visit_violation(), None);
        if minute >= 240 && !cut {
            // Cut the path between the zebras and the giraffes when nobody stands on it.
            cut = state.bulldoze(14, 7).ok;
        }
        if minute == 360 {
            assert!(state.place_path(14, 7).ok);
        }
        if let Some(stuck) = state.guests.iter().find(|g| g.minutes_in_park > 600) {
            panic!("guest {} stayed {} minutes: {stuck:?}", stuck.id, stuck.minutes_in_park);
        }
    }
    assert!(cut, "the path was cut");
    let isolated = state.concessions.iter().find(|c| c.x == 18 && c.y == 1).unwrap();
    assert_eq!(isolated.total_uses, 0);
    assert!(state.visits.total.total() > 20);
}

#[test]
fn groups_pay_one_admission_line_per_person_and_count_only_real_guests() {
    let (mut state, _) = itinerary_park(true);
    for _ in 0..720 {
        state.tick(1);
        let in_groups: usize = state.groups.iter().map(|g| g.members.len()).sum();
        assert_eq!(in_groups, state.guests.len());
        let arrived = state.next_guest_id - 1;
        assert_eq!(
            state.finance_today.admissions_income_cents,
            i64::from(arrived) * ADMISSION_PRICE
        );
        // Everyone who came is in the park or walked out with a reason.
        assert_eq!(
            state.guests.len() as u32 + state.visits.total.total(),
            arrived
        );
    }
    let events = |kind: &str| {
        state
            .pending_events
            .iter()
            .filter(|event| event.kind == kind)
            .count() as u32
    };
    let arrived = state.next_guest_id - 1;
    assert_eq!(events("guest_arrived"), arrived);
    assert_eq!(events("group_arrived"), state.visits.groups_arrived);
    assert!(state.visits.groups_arrived < arrived, "groups of several people came");
    // The summary counts people from the admission ledger.
    let summary = serde_json::to_value(state.daily_summary()).unwrap();
    assert_eq!(summary["admissions"], arrived);
    assert_eq!(summary["exits"], state.visits.today.total());
}

#[test]
fn exit_reasons_name_the_state_behind_them() {
    let (mut state, _) = itinerary_park(false);
    let members = spawn(&mut state, visit::arrival_of_size(2, 1));
    let index = state.guests.iter().position(|g| g.id == members[1]).unwrap();
    let profile = state.groups[0].profile;
    let guest = &mut state.guests[index];
    guest.crowd_minutes = profile.rules().patience;
    let cause = state.blocking_cause(&state.guests[index], profile).unwrap();
    assert_eq!(cause.reason, visit::ExitReason::TooCrowded);
    assert!(cause.detail.contains("minutes in crowds"), "{}", cause.detail);
    state.guests[index].thirst = GUEST_CONTINUE_MAX_THIRST + 1;
    state.guests[index].unmet_need = Some(comfort::Need::Drink);
    let cause = state.blocking_cause(&state.guests[index], profile).unwrap();
    assert_eq!(cause.reason, visit::ExitReason::NeedsUnmet);
    assert_eq!(
        cause.detail,
        format!("Guest #{} is thirsty and the park could not help.", members[1])
    );
    state.guests[index].thirst = 0;
    state.guests[index].crowd_minutes = 0;
    state.guests[index].value_perception = GUEST_CONTINUE_MIN_VALUE - 1;
    let cause = state.blocking_cause(&state.guests[index], profile).unwrap();
    assert_eq!(cause.reason, visit::ExitReason::PoorValue);
    // The whole group leaves together, each with the reason that ended the visit.
    for guest in &mut state.guests {
        guest.state = GuestState::Viewing;
        guest.viewing_minutes = 0;
    }
    state.advance_groups();
    for guest in &state.guests {
        assert_eq!(guest.state, GuestState::WalkingToExit);
        assert_eq!(guest.exit.as_ref().unwrap().reason, visit::ExitReason::PoorValue);
    }
    assert_eq!(
        state.guest_thought(&state.guests[0]),
        "This wasn't worth the price; I'm leaving."
    );
}
