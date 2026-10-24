//! Habitat service gates (#95 gate/access slice).
//!
//! Every habitat owns one fence segment that keepers walk through. A new habitat gets the
//! segment whose outside tile is nearest the animal-care depot by path, falling back to the
//! first usable segment clockwise from the north-west corner. The tile outside the gate stays
//! unbuildable; losing the path to it is allowed and only blocks care ("Gate unreachable").
use super::{ActionResult, FenceSegment, FenceSide, GameState, Habitat, Position, TileKind};
use serde::Serialize;

pub(crate) fn gate_access_message(habitat_id: u32) -> String {
    format!("Keep this tile clear: it is the service gate access for habitat #{habitat_id}")
}

/// A gate-move preview: the fence segment a tapped tile resolves to and whether it is usable.
#[derive(Serialize)]
pub(crate) struct GateEvaluation {
    ok: bool,
    message: String,
    segment: Option<FenceSegment>,
    access: Option<Position>,
    /// The work zone the habitat would belong to with its gate on this segment.
    zone_id: Option<u32>,
}

#[cfg(test)]
impl GateEvaluation {
    pub(crate) fn zone_id(&self) -> Option<u32> {
        self.zone_id
    }
}

/// The habitat's fence segments in clockwise order starting at its north-west corner.
fn clockwise_segments(x: u32, y: u32, width: u32, height: u32) -> Vec<FenceSegment> {
    let right = x + width - 1;
    let bottom = y + height - 1;
    let north = (x..=right).map(|tile_x| FenceSegment {
        x: tile_x,
        y,
        side: FenceSide::North,
    });
    let east = (y..=bottom).map(|tile_y| FenceSegment {
        x: right,
        y: tile_y,
        side: FenceSide::East,
    });
    let south = (x..=right).rev().map(|tile_x| FenceSegment {
        x: tile_x,
        y: bottom,
        side: FenceSide::South,
    });
    let west = (y..=bottom).rev().map(|tile_y| FenceSegment {
        x,
        y: tile_y,
        side: FenceSide::West,
    });
    north.chain(east).chain(south).chain(west).collect()
}

impl GameState {
    pub(crate) fn depot_position(&self) -> Position {
        self.depot
    }

    pub(crate) fn depot_access_tiles(&self) -> Vec<Position> {
        self.neighbors(self.depot_position())
    }

    /// The tile just outside a fence segment, when it lies inside the park.
    pub(crate) fn gate_access_tile(&self, segment: FenceSegment) -> Option<Position> {
        let (x, y) = match segment.side {
            FenceSide::North => (Some(segment.x), segment.y.checked_sub(1)),
            FenceSide::South => (Some(segment.x), segment.y.checked_add(1)),
            FenceSide::West => (segment.x.checked_sub(1), Some(segment.y)),
            FenceSide::East => (segment.x.checked_add(1), Some(segment.y)),
        };
        let position = Position { x: x?, y: y? };
        self.tile(position.x, position.y).map(|_| position)
    }

    /// A gate can open onto a path or open grass inside the park, never onto a building.
    pub(crate) fn gate_segment_usable(&self, segment: FenceSegment) -> bool {
        self.gate_access_tile(segment).is_some_and(|access| {
            matches!(
                self.tile(access.x, access.y),
                Some(TileKind::Grass | TileKind::Path | TileKind::Entrance)
            )
        })
    }

    /// The habitat whose service gate opens onto `position`, if any.
    pub(crate) fn service_gate_owner(&self, position: Position) -> Option<u32> {
        self.habitats
            .iter()
            .find(|habitat| self.gate_access_tile(habitat.service_gate) == Some(position))
            .map(|habitat| habitat.id)
    }

    /// Inside and outside tiles of the habitat's gate.
    pub(crate) fn service_gate_tiles(&self, habitat: &Habitat) -> Option<(Position, Position)> {
        let inside = Position {
            x: habitat.service_gate.x,
            y: habitat.service_gate.y,
        };
        Some((inside, self.gate_access_tile(habitat.service_gate)?))
    }

    /// Whether a keeper leaving the depot can walk along paths to the gate's outside tile.
    pub(crate) fn service_gate_reachable(&self, habitat: &Habitat) -> bool {
        self.gate_access_tile(habitat.service_gate)
            .is_some_and(|access| {
                self.search_path(access, &self.depot_access_tiles())
                    .is_some()
            })
    }

    pub(crate) fn default_service_gate(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> FenceSegment {
        let segments = clockwise_segments(x, y, width, height);
        let depot_access = self.depot_access_tiles();
        let nearest = segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| self.gate_segment_usable(**segment))
            .filter_map(|(order, segment)| {
                let access = self.gate_access_tile(*segment)?;
                let route = self.path_to_any(access, &depot_access)?;
                Some((route.len(), order, *segment))
            })
            .min_by_key(|(length, order, _)| (*length, *order))
            .map(|(_, _, segment)| segment);
        nearest
            .or_else(|| {
                segments
                    .iter()
                    .copied()
                    .find(|segment| self.gate_segment_usable(*segment))
            })
            .unwrap_or(segments[0])
    }

    pub(crate) fn evaluate_service_gate(&self, habitat_id: u32, x: u32, y: u32) -> GateEvaluation {
        let rejected = |message: String, segment: Option<FenceSegment>| GateEvaluation {
            ok: false,
            message,
            segment,
            access: segment.and_then(|segment| self.gate_access_tile(segment)),
            zone_id: None,
        };
        let Some(habitat) = self.habitats.iter().find(|h| h.id == habitat_id) else {
            return rejected("Select a habitat first".to_owned(), None);
        };
        let tapped = Position { x, y };
        // A tapped border tile or the tile just outside it selects that segment; usable and
        // path-facing segments win at corners, then clockwise order.
        let candidates: Vec<(usize, FenceSegment)> =
            clockwise_segments(habitat.x, habitat.y, habitat.width, habitat.height)
                .into_iter()
                .enumerate()
                .filter(|(_, segment)| {
                    Position {
                        x: segment.x,
                        y: segment.y,
                    } == tapped
                        || self.gate_access_tile(*segment) == Some(tapped)
                })
                .collect();
        let Some((_, segment)) = candidates.into_iter().min_by_key(|(order, segment)| {
            let walkable = self
                .gate_access_tile(*segment)
                .is_some_and(|access| self.is_walkable(access));
            (!self.gate_segment_usable(*segment), !walkable, *order)
        }) else {
            return rejected(
                format!("Tap a fence segment of habitat #{habitat_id}"),
                None,
            );
        };
        if let Err(message) = self.validate_service_gate(habitat, segment) {
            return rejected(message, Some(segment));
        }
        let zone_id = self.zone_at(Position {
            x: segment.x,
            y: segment.y,
        });
        let zone = zone_id.map_or_else(
            || "no work zone".to_owned(),
            |zone_id| format!("work zone #{zone_id}"),
        );
        GateEvaluation {
            ok: true,
            message: if segment == habitat.service_gate {
                format!("The service gate is already here · {zone}")
            } else {
                format!("Move the service gate here · {zone}")
            },
            segment: Some(segment),
            access: self.gate_access_tile(segment),
            zone_id,
        }
    }

    fn validate_service_gate(
        &self,
        habitat: &Habitat,
        segment: FenceSegment,
    ) -> Result<(), String> {
        if !habitat.fence_segments().contains(&segment) {
            return Err(format!(
                "That is not a fence segment of habitat #{}",
                habitat.id
            ));
        }
        if !self.gate_segment_usable(segment) {
            return Err(
                "The service gate must open onto a path or open grass inside the park".to_owned(),
            );
        }
        Ok(())
    }

    pub(crate) fn set_service_gate(
        &mut self,
        habitat_id: u32,
        x: u32,
        y: u32,
        side: &str,
    ) -> ActionResult {
        let Some(side) = FenceSide::parse(side) else {
            return ActionResult::error("Unknown fence side");
        };
        let segment = FenceSegment { x, y, side };
        let Some(index) = self.habitats.iter().position(|h| h.id == habitat_id) else {
            return ActionResult::error("Select a habitat first");
        };
        if self.habitats[index].service_gate == segment {
            return ActionResult::ok("The service gate is already here");
        }
        if let Err(message) = self.validate_service_gate(&self.habitats[index], segment) {
            return ActionResult::error(message);
        }
        self.habitats[index].service_gate = segment;
        ActionResult::ok(format!("Habitat #{habitat_id} service gate moved"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ENTRANCE_Y, HabitatOrientation};

    #[test]
    fn clockwise_order_starts_north_west_and_walks_the_perimeter() {
        let order: Vec<_> = clockwise_segments(1, 1, 2, 2)
            .into_iter()
            .map(|segment| (segment.x, segment.y, segment.side))
            .collect();
        assert_eq!(
            order,
            vec![
                (1, 1, FenceSide::North),
                (2, 1, FenceSide::North),
                (2, 1, FenceSide::East),
                (2, 2, FenceSide::East),
                (2, 2, FenceSide::South),
                (1, 2, FenceSide::South),
                (1, 2, FenceSide::West),
                (1, 1, FenceSide::West),
            ]
        );
    }

    #[test]
    fn default_gate_faces_the_path_nearest_the_depot() {
        let mut state = GameState::default();
        // Entrance path runs along y = 7 from x = 1 to 4; the depot sits at (2, 6).
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let gate = state.habitats[0].service_gate;
        assert_eq!((gate.x, gate.y, gate.side), (3, 8, FenceSide::North));
        assert_eq!(
            state.gate_access_tile(gate),
            Some(Position {
                x: 3,
                y: ENTRANCE_Y
            })
        );
        assert!(state.service_gate_reachable(&state.habitats[0]));
    }

    #[test]
    fn unreachable_habitat_falls_back_to_first_usable_segment_clockwise_from_north() {
        let mut state = GameState::default();
        for x in 5..=12 {
            assert!(state.place_path(x, ENTRANCE_Y).ok);
        }
        // Cut the path so the habitat's only adjacent path is disconnected from the depot.
        assert!(state.bulldoze(5, ENTRANCE_Y).ok);
        assert!(state.place_habitat_rect(9, 8, 11, 10).ok);
        let gate = state.habitats[0].service_gate;
        assert_eq!((gate.x, gate.y, gate.side), (9, 8, FenceSide::North));
        assert!(!state.service_gate_reachable(&state.habitats[0]));
    }

    #[test]
    fn gate_moves_only_onto_usable_segments_of_its_own_habitat() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let id = state.habitats[0].id;

        let moved = state.set_service_gate(id, 6, 9, "east");
        assert!(moved.ok, "{}", moved.message);
        assert!(state.set_service_gate(id, 6, 9, "east").ok);
        assert!(
            !state.set_service_gate(id, 4, 9, "east").ok,
            "interior edge"
        );
        assert!(!state.set_service_gate(id, 6, 9, "up").ok);
        assert!(!state.set_service_gate(99, 6, 9, "east").ok);

        assert_eq!(state.habitats[0].service_gate.side, FenceSide::East);

        // A park-boundary segment has no outside tile, and a building blocks the other.
        for y in 11..=13 {
            assert!(state.place_path(7, y).ok);
        }
        assert!(state.place_habitat_rect(8, 11, 10, 13).ok);
        let edge = state.habitats[1].id;
        let boundary = state.set_service_gate(edge, 9, 13, "south");
        assert!(!boundary.ok);
        assert!(boundary.message.contains("must open onto"));
        assert!(state.place_path(12, 12).ok);
        assert!(state.place_concession(11, 12, "food").ok);
        assert!(!state.set_service_gate(edge, 10, 12, "east").ok);
        assert!(state.set_service_gate(edge, 10, 11, "east").ok);
    }

    #[test]
    fn nothing_can_be_built_on_the_tile_outside_a_gate() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let id = state.habitats[0].id;
        assert!(state.set_service_gate(id, 4, 10, "south").ok);
        let cash = state.cash_cents;

        let stand = state.place_concession(4, 11, "food");
        assert!(!stand.ok);
        assert_eq!(stand.message, gate_access_message(id));
        let habitat = state.place_habitat_rect(3, 11, 5, 13);
        assert!(!habitat.ok);
        assert_eq!(habitat.message, gate_access_message(id));
        assert_eq!(state.cash_cents, cash);
        // Paths are what keepers walk on, so they stay allowed.
        assert!(state.place_path(4, 11).ok);
    }

    #[test]
    fn tapped_tiles_resolve_to_a_segment_preferring_path_facing_corners() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let id = state.habitats[0].id;
        // (3, 8) is the north-west corner: north faces the path, west faces grass.
        let corner = state.evaluate_service_gate(id, 3, 8);
        assert!(corner.ok);
        assert_eq!(corner.segment.unwrap().side, FenceSide::North);
        // The outside tile selects the segment it faces.
        let outside = state.evaluate_service_gate(id, 2, 9);
        assert_eq!(outside.segment.unwrap().side, FenceSide::West);
        assert!(!state.evaluate_service_gate(id, 12, 12).ok);
    }
}
