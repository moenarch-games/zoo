// Park circulation (#98): wide paths, plazas, staff-only routes and explicit service access.
// The default park is 20x14 with the entrance at (0, 7), the depot at (2, 6) and a public path
// along y = 7 from x = 1 to 4.
use super::*;

fn at(x: u32, y: u32) -> Position {
    Position { x, y }
}

fn access_at(state: &GameState, x: u32, y: u32) -> Option<PathAccess> {
    match state.tile(x, y) {
        Some(TileKind::Path(access)) => Some(access),
        _ => None,
    }
}

fn staff_tiles(state: &GameState, route: &[Position]) -> Vec<Position> {
    route
        .iter()
        .copied()
        .filter(|tile| access_at(state, tile.x, tile.y) == Some(PathAccess::Staff))
        .collect()
}

fn stroke(state: &mut GameState, points: &[u32], width: u32, access: &str) {
    let result = state.build_path_stroke(points, width, access);
    assert!(result.ok, "{}", result.message);
}

/// A stocked habitat east of the entrance, reachable by guests only over a long public detour
/// and by staff over a short staff-only lane.
fn shortcut_park() -> (GameState, u32) {
    let mut state = GameState::default();
    // Staff-only lane straight east from the public path to the habitat's north side.
    stroke(&mut state, &[5, 7, 9, 7], 1, "staff");
    // Public viewing path along the north fence, then a detour around the south.
    stroke(&mut state, &[10, 7, 13, 7, 13, 12, 4, 12, 4, 8], 1, "public");
    assert!(state.place_habitat_rect(10, 8, 12, 10).ok);
    let id = state.habitats[0].id;
    staff_habitat(&mut state, id);
    assert!(state.adopt(id, "capybara").ok);
    (state, id)
}

#[test]
fn guests_never_take_a_staff_only_shortcut_but_staff_do() {
    let (state, id) = shortcut_park();
    let habitat = state.habitats.iter().find(|h| h.id == id).unwrap();

    let guest_route = state.viewing_route(habitat, state.entrance).expect("public detour");
    assert!(staff_tiles(&state, &guest_route).is_empty(), "{guest_route:?}");
    assert!(guest_route.contains(&at(4, 12)), "guests walk the detour");

    // The service gate faces the staff lane and staff reach it over the shortcut.
    let gate_access = state.gate_access_tile(habitat.service_gate).unwrap();
    let staff_route = state
        .search_path(Mover::Staff, gate_access, &state.depot_access_tiles())
        .expect("staff reach the gate");
    assert!(!staff_tiles(&state, &staff_route).is_empty(), "{staff_route:?}");
    assert!(staff_route.len() < guest_route.len());
    assert!(state.service_gate_reachable(habitat));

    // A staff-only tile is never a guest viewpoint.
    let occupancy = state.viewing_occupancy();
    for spot in state.viewing_spots_with_occupancy(habitat, &occupancy) {
        assert_eq!(access_at(&state, spot.position.x, spot.position.y), Some(PathAccess::Public));
    }
}

#[test]
fn cutting_the_public_detour_blocks_guests_instead_of_opening_the_shortcut() {
    let (mut state, _) = shortcut_park();
    for _ in 0..200 {
        state.tick(1);
        if !state.guests.is_empty() {
            break;
        }
    }
    assert!(!state.guests.is_empty(), "a guest arrives");
    let before = state.work.route_invalidations;
    assert!(state.bulldoze(4, 10).ok, "cut the detour");
    let guest = &state.guests[0];
    assert!(guest.route.is_empty(), "the guest waits rather than using staff paths");
    assert!(state.work.route_invalidations > before);
    // Repairing the connection restores a public route.
    stroke(&mut state, &[4, 10], 1, "public");
    let guest = &state.guests[0];
    assert!(!guest.route.is_empty(), "repair re-plans the route");
    assert!(staff_tiles(&state, &guest.route[guest.route_index..]).is_empty());
    for _ in 0..120 {
        state.tick(1);
        for guest in &state.guests {
            assert_ne!(access_at(&state, guest.x, guest.y), Some(PathAccess::Staff));
        }
    }
}

#[test]
fn a_wider_brush_builds_real_tiles_at_the_tile_price() {
    let mut state = GameState::default();
    let cash = state.cash_cents;
    let preview = state.evaluate_path_stroke(&[5, 7, 8, 7], 2, "public");
    assert!(preview.ok && preview.changes, "{}", preview.message);
    assert_eq!(preview.tiles.len(), 10, "even widths extend right and down: 5x2");
    assert_eq!(preview.cost_cents, 10 * PATH_COST);
    stroke(&mut state, &[5, 7, 8, 7], 2, "public");
    assert_eq!(state.cash_cents, cash - 10 * PATH_COST);
    for x in 5..=9 {
        for y in 7..=8 {
            assert_eq!(access_at(&state, x, y), Some(PathAccess::Public), "{x},{y}");
        }
    }

    // Widening an existing lane charges only the new tiles.
    stroke(&mut state, &[10, 3, 12, 3], 1, "public");
    let cash = state.cash_cents;
    let widened = state.evaluate_path_stroke(&[10, 3, 12, 3], 3, "public");
    assert_eq!(widened.tiles.len(), 12, "a 5x3 band minus the 3 existing tiles");
    stroke(&mut state, &[10, 3, 12, 3], 3, "public");
    assert_eq!(state.cash_cents, cash - 12 * PATH_COST);

    // A brush is clipped at the park edge, and widths outside 1..=3 are refused.
    let edge = state.evaluate_path_stroke(&[19, 0], 3, "public");
    assert!(edge.ok);
    assert_eq!(edge.tiles.len(), 4);
    assert!(!state.evaluate_path_stroke(&[5, 2], 4, "public").ok);
    assert!(!state.evaluate_path_stroke(&[5, 2], 0, "public").ok);
    assert!(!state.evaluate_path_stroke(&[5, 2], 1, "vip").ok);
}

#[test]
fn width_adds_viewpoints_without_multiplying_a_viewpoint_capacity() {
    let mut state = GameState::default();
    // A three-wide promenade along the habitat's north fence.
    stroke(&mut state, &[5, 7, 14, 7], 3, "public");
    assert!(state.place_habitat_rect(8, 9, 12, 11).ok);
    let id = state.habitats[0].id;
    staff_habitat(&mut state, id);
    for _ in 0..4 {
        assert!(state.adopt(id, "capybara").ok);
    }
    let habitat = &state.habitats[0];
    let occupancy = state.viewing_occupancy();
    let spots = state.viewing_spots_with_occupancy(habitat, &occupancy);
    assert!(!spots.is_empty());
    for spot in &spots {
        assert!(spot.capacity <= VIEWPOINT_PHYSICAL_CAPACITY);
        // Only the row touching the fence views over it; the wider rows are walking space.
        assert_eq!(spot.position.y, 8);
    }
}

#[test]
fn plazas_fill_a_rectangle_as_one_transaction() {
    let mut state = GameState::default();
    let cash = state.cash_cents;
    let plaza = state.build_plaza(6, 2, 8, 4, "public");
    assert!(plaza.ok, "{}", plaza.message);
    assert_eq!(state.cash_cents, cash - 9 * PATH_COST);
    assert!(state.place_habitat_rect(9, 2, 11, 4).ok);
    let cash = state.cash_cents;
    let blocked = state.build_plaza(10, 0, 10, 2, "public");
    assert!(!blocked.ok);
    assert!(blocked.message.contains("A habitat blocks"), "{}", blocked.message);
    assert_eq!(state.cash_cents, cash);
    assert_eq!(state.tile(10, 0), Some(TileKind::Grass), "nothing is built");
    assert!(!state.evaluate_plaza(0, 0, 19, 13, "public").ok, "too large");
    // The plaza undoes as a whole.
    assert!(state.undo_construction().ok);
    assert!(state.undo_construction().ok);
    assert_eq!(state.tile(7, 3), Some(TileKind::Grass));
}

#[test]
fn reclassifying_is_free_undoable_and_refused_under_a_guest() {
    let mut state = GameState::default();
    stroke(&mut state, &[5, 7, 9, 7], 1, "public");
    let cash = state.cash_cents;
    let preview = state.evaluate_path_stroke(&[6, 7, 7, 7], 1, "staff");
    assert!(preview.ok && preview.changes, "{}", preview.message);
    assert_eq!(preview.cost_cents, 0);
    stroke(&mut state, &[6, 7, 7, 7], 1, "staff");
    assert_eq!(state.cash_cents, cash);
    assert_eq!(access_at(&state, 6, 7), Some(PathAccess::Staff));
    assert!(state.undo_construction().ok);
    assert_eq!(access_at(&state, 6, 7), Some(PathAccess::Public));
    assert!(state.redo_construction().ok);
    assert_eq!(access_at(&state, 7, 7), Some(PathAccess::Staff));
    assert!(state.undo_construction().ok);

    // A guest standing on a tile keeps it public.
    let (mut state, _) = shortcut_park();
    for _ in 0..200 {
        state.tick(1);
        if state.guests.iter().any(|g| g.x > 0) {
            break;
        }
    }
    let guest = state.guests.iter().find(|g| g.x > 0).expect("a walking guest");
    let (x, y) = (guest.x, guest.y);
    let cash = state.cash_cents;
    let refused = state.build_path_stroke(&[x, y], 1, "staff");
    assert!(!refused.ok);
    assert!(refused.message.contains("A guest is standing"), "{}", refused.message);
    assert_eq!(state.cash_cents, cash);
    assert_eq!(access_at(&state, x, y), Some(PathAccess::Public));
}

#[test]
fn an_access_change_re_plans_exactly_the_routes_it_breaks() {
    let (mut state, _) = shortcut_park();
    for _ in 0..200 {
        state.tick(1);
        if state.guests.iter().any(|g| g.route.len() > g.route_index + 4) {
            break;
        }
    }
    let guest = state
        .guests
        .iter()
        .find(|g| g.route.len() > g.route_index + 4)
        .expect("a guest with a long walk ahead")
        .clone();

    // An unrelated edit advances the topology revision and keeps every route.
    let (revision, invalidated) = (state.work.topology_revision, state.work.route_invalidations);
    stroke(&mut state, &[17, 1, 18, 1], 1, "public");
    assert_eq!(state.work.topology_revision, revision + 1);
    assert_eq!(state.work.route_invalidations, invalidated);
    assert_eq!(state.guests.iter().find(|g| g.id == guest.id).unwrap().route, guest.route);

    // Turning a tile ahead of the guest staff-only invalidates that route only.
    let ahead = guest.route[guest.route_index + 3];
    assert!(state.guests.iter().all(|g| g.x != ahead.x || g.y != ahead.y));
    stroke(&mut state, &[ahead.x, ahead.y], 1, "staff");
    assert_eq!(state.work.topology_revision, revision + 2);
    assert!(state.work.route_invalidations > invalidated);
    let replanned = state.guests.iter().find(|g| g.id == guest.id).unwrap();
    assert!(!replanned.route[replanned.route_index..].contains(&ahead));
}

#[test]
fn stands_use_their_nearest_reachable_service_point_and_need_a_public_side() {
    let mut state = GameState::default();
    stroke(&mut state, &[5, 7, 8, 7], 1, "public");
    // A staff lane from the depot's east side, behind the stand.
    stroke(&mut state, &[3, 6, 3, 5, 6, 5], 1, "staff");
    assert!(state.place_concession(6, 6, "food").ok);
    let id = state.concessions[0].id;
    assert_eq!(state.concession_customer_tiles(id), vec![at(6, 7)]);
    let (points, active) = state.concession_service_view(id);
    assert_eq!(points.len(), 2);
    assert!(points.iter().all(|point| point.reachable));
    assert_eq!(active, Some(at(6, 5)), "equal walks prefer the northern point");

    // Cutting the staff lane leaves the public side as the alternative service entrance.
    assert!(state.bulldoze(4, 5).ok);
    let (points, active) = state.concession_service_view(id);
    assert!(points.iter().any(|point| !point.reachable));
    assert_eq!(active, Some(at(6, 7)));
    stroke(&mut state, &[4, 5], 1, "staff");
    assert_eq!(state.concession_service_view(id).1, Some(at(6, 5)));

    // A stand touching only a staff-only path has no customers and is refused.
    stroke(&mut state, &[12, 2, 14, 2], 1, "staff");
    let refused = state.place_concession(13, 3, "drink");
    assert!(!refused.ok);
    assert!(refused.message.contains("public"), "{}", refused.message);

    // Reclassifying a stand's last public tile is allowed but previewed as a loss.
    let preview = state.evaluate_path_stroke(&[6, 7], 1, "staff");
    assert!(preview.ok);
    assert_eq!(preview.affected.len(), 1, "{:?}", preview.affected);
}

#[test]
fn mechanics_reach_a_stand_through_its_staff_only_back() {
    let mut state = GameState::default();
    stroke(&mut state, &[5, 7, 8, 7], 1, "public");
    stroke(&mut state, &[3, 6, 3, 5, 6, 5], 1, "staff");
    assert!(state.place_concession(6, 6, "food").ok);
    assert!(state.hire_mechanic().ok);
    // Cut the public side: only the staff-only back reaches the stand.
    assert!(state.bulldoze(5, 7).ok);
    let id = state.concessions[0].id;
    state.concessions[0].condition = MAINTENANCE_SERVICE_THRESHOLD - 1;
    for _ in 0..240 {
        state.tick(1);
        if state.concessions[0].condition == 100 {
            break;
        }
    }
    assert_eq!(state.concessions[0].condition, 100, "stand #{id} repaired from the back");
}
