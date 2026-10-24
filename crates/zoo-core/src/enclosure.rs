//! Habitat enclosure plans (#95): building a fence loop and reshaping an existing one.
//!
//! Rectangles, drawn outlines and reshapes all reduce to one proposed tile set, which a single
//! plan checks against the park and prices. Committing plans again (see `construction.rs`).
//!
//! Reshaping keeps the habitat's ID, animals, care state, staff assignments and references. It
//! charges for the tiles and fence segments it adds and refunds nothing it removes, like
//! demolition. It rejects a fence that would hold fewer animals than live there, that leaves
//! out a tile a keeper stands on, or that covers another habitat's gate access. The service
//! gate stays where it is while its segment is still part of the new fence; otherwise the plan
//! picks the default gate for the new shape and says so.
use super::{
    FenceSegment, GameState, HabitatOrientation, MAX_HABITAT_AREA, MIN_HABITAT_DIMENSION, Position,
    TileKind, content::FenceType, gate::gate_access_message, shape,
};
use serde::Serialize;

/// Any closed fence around at most [`MAX_HABITAT_AREA`] tiles is at most this long.
const MAX_FENCE_EDGES: usize = 2 * MAX_HABITAT_AREA as usize + 2;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PlacementEvaluation {
    pub ok: bool,
    pub message: String,
    /// The habitat being reshaped; `None` for a new enclosure.
    pub habitat_id: Option<u32>,
    /// The enclosure's bounding box.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub area: u32,
    pub perimeter: u32,
    pub capacity: u32,
    pub cost_cents: i64,
    /// The proposed enclosure, row-major.
    pub occupied_tiles: Vec<Position>,
    pub fence_segments: Vec<FenceSegment>,
    /// For a reshape: tiles the new fence adds and leaves out.
    pub added_tiles: Vec<Position>,
    pub removed_tiles: Vec<Position>,
    /// The service gate the enclosure would have.
    pub service_gate: Option<FenceSegment>,
    /// Consequences that do not block the plan.
    pub warnings: Vec<String>,
    /// The grid corner where a drawn outline fails, if it does.
    pub blocked_corner: Option<Position>,
}

impl PlacementEvaluation {
    /// A rejection before the proposal is a bounded enclosure inside the park, so no geometry
    /// is materialized for it.
    fn invalid(message: impl Into<String>, habitat_id: Option<u32>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            habitat_id,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            area: 0,
            perimeter: 0,
            capacity: 0,
            cost_cents: 0,
            occupied_tiles: Vec::new(),
            fence_segments: Vec::new(),
            added_tiles: Vec::new(),
            removed_tiles: Vec::new(),
            service_gate: None,
            warnings: Vec::new(),
            blocked_corner: None,
        }
    }

    fn reject(mut self, message: impl Into<String>) -> Self {
        self.ok = false;
        self.message = message.into();
        self
    }
}

/// The animals an enclosure of `area` tiles holds.
pub(crate) fn capacity_for(area: u32) -> u32 {
    (area / 3).clamp(2, 12)
}

impl GameState {
    /// The fence every new habitat is built with: the catalog's first fence type.
    pub(crate) fn default_fence(&self) -> &FenceType {
        &self.catalog.fences[0]
    }

    /// The catalog price of a new enclosure.
    pub(crate) fn habitat_cost(&self, area: u32, perimeter: u32) -> i64 {
        let costs = &self.catalog.balance.construction;
        costs
            .habitat_base_cents
            .saturating_add(i64::from(area).saturating_mul(costs.habitat_tile_cents))
            .saturating_add(i64::from(perimeter).saturating_mul(self.default_fence().segment_cents))
    }

    /// The price of reshaping: new tiles and new fence segments only.
    fn reshape_cost(&self, added_tiles: usize, added_segments: usize, segment_cents: i64) -> i64 {
        let costs = &self.catalog.balance.construction;
        let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
        count(added_tiles)
            .saturating_mul(costs.habitat_tile_cents)
            .saturating_add(count(added_segments).saturating_mul(segment_cents))
    }

    /// Every rectangle evaluation, accepted or not, previews the price of its fence loop.
    pub(crate) fn evaluate_habitat_rect(
        &self,
        ax: u32,
        ay: u32,
        bx: u32,
        by: u32,
    ) -> PlacementEvaluation {
        let (x, y, width, height) = super::normalized_rect(ax, ay, bx, by);
        let preview = |message: String| {
            let perimeter = width.saturating_add(height).saturating_mul(2);
            PlacementEvaluation {
                x,
                y,
                width,
                height,
                area: width.saturating_mul(height),
                perimeter,
                cost_cents: self.habitat_cost(width.saturating_mul(height), perimeter),
                ..PlacementEvaluation::invalid(message, None)
            }
        };
        if width < MIN_HABITAT_DIMENSION || height < MIN_HABITAT_DIMENSION {
            return preview(format!(
                "Draw at least a {MIN_HABITAT_DIMENSION}×{MIN_HABITAT_DIMENSION} fence loop"
            ));
        }
        let area = u64::from(width).saturating_mul(u64::from(height));
        if area > MAX_HABITAT_AREA {
            return preview(format!(
                "This fence encloses {area} tiles; the current limit is {MAX_HABITAT_AREA}"
            ));
        }
        let in_bounds = x
            .checked_add(width)
            .is_some_and(|right| right <= self.width)
            && y.checked_add(height)
                .is_some_and(|bottom| bottom <= self.height);
        if !in_bounds {
            return preview("The fence would extend outside the park".to_owned());
        }
        self.evaluate_enclosure(shape::rect_tiles(x, y, width, height), None)
    }

    pub(crate) fn evaluate_habitat(
        &self,
        x: u32,
        y: u32,
        orientation: HabitatOrientation,
    ) -> PlacementEvaluation {
        let (width, height) = orientation.dimensions();
        self.evaluate_habitat_rect(
            x,
            y,
            x.saturating_add(width - 1),
            y.saturating_add(height - 1),
        )
    }

    /// The plan for the enclosure a drawn outline (flat `x, y` corner pairs) proposes.
    fn evaluate_outline(&self, points: &[u32], habitat_id: Option<u32>) -> PlacementEvaluation {
        let tiles = match shape::outline_tiles(points, self.width, self.height, MAX_FENCE_EDGES) {
            Ok(tiles) => tiles,
            Err(error) => {
                return PlacementEvaluation {
                    blocked_corner: error.corner,
                    ..PlacementEvaluation::invalid(error.message, habitat_id)
                };
            }
        };
        let area = tiles.len() as u64;
        if area > MAX_HABITAT_AREA {
            return PlacementEvaluation::invalid(
                format!(
                    "This fence encloses {area} tiles; the current limit is {MAX_HABITAT_AREA}"
                ),
                habitat_id,
            );
        }
        self.evaluate_enclosure(tiles, habitat_id)
    }

    /// Previews a new habitat inside a drawn outline exactly as committing it would build it.
    pub(crate) fn evaluate_habitat_outline(&self, points: &[u32]) -> PlacementEvaluation {
        self.evaluate_outline(points, None)
    }

    /// Previews moving habitat `habitat_id`'s fence onto a drawn outline.
    pub(crate) fn evaluate_habitat_reshape(
        &self,
        habitat_id: u32,
        points: &[u32],
    ) -> PlacementEvaluation {
        if !self.habitats.iter().any(|h| h.id == habitat_id) {
            return PlacementEvaluation::invalid("Select a habitat first", Some(habitat_id));
        }
        self.evaluate_outline(points, Some(habitat_id))
    }

    /// The plan for `tiles` with the cash check committing needs.
    pub(crate) fn evaluate_enclosure(
        &self,
        tiles: Vec<Position>,
        habitat_id: Option<u32>,
    ) -> PlacementEvaluation {
        let plan = self.plan_enclosure(tiles, habitat_id);
        if plan.ok && self.cash_cents < plan.cost_cents {
            return plan.reject("Not enough cash");
        }
        plan
    }

    /// Checks a proposed enclosure (row-major tiles inside the park) against the park and
    /// prices it; reshaping `habitat_id` may reuse that habitat's own tiles.
    pub(crate) fn plan_enclosure(
        &self,
        tiles: Vec<Position>,
        habitat_id: Option<u32>,
    ) -> PlacementEvaluation {
        let habitat = habitat_id.and_then(|id| self.habitats.iter().find(|h| h.id == id));
        let Some(fence) = shape::clockwise_fence(&tiles) else {
            return PlacementEvaluation::invalid(
                "The fence must enclose one connected area",
                habitat_id,
            );
        };
        let (x, y, width, height) = shape::bounds(&tiles);
        let area = u32::try_from(tiles.len()).unwrap_or(u32::MAX);
        let perimeter = u32::try_from(fence.len()).unwrap_or(u32::MAX);
        let (added_tiles, removed_tiles, cost_cents) = match habitat {
            Some(habitat) => {
                let added: Vec<Position> = tiles
                    .iter()
                    .copied()
                    .filter(|tile| !habitat.contains(*tile))
                    .collect();
                let removed: Vec<Position> = habitat
                    .tiles
                    .iter()
                    .copied()
                    .filter(|tile| !shape::contains(&tiles, *tile))
                    .collect();
                let old_fence = habitat.fence_segments();
                let new_segments = fence.iter().filter(|s| !old_fence.contains(s)).count();
                let cost =
                    self.reshape_cost(added.len(), new_segments, habitat.fence.segment_cents);
                (added, removed, cost)
            }
            None => (Vec::new(), Vec::new(), self.habitat_cost(area, perimeter)),
        };
        let mut plan = PlacementEvaluation {
            ok: true,
            message: String::new(),
            habitat_id,
            x,
            y,
            width,
            height,
            area,
            perimeter,
            capacity: capacity_for(area),
            cost_cents,
            occupied_tiles: tiles,
            fence_segments: fence,
            added_tiles,
            removed_tiles,
            service_gate: None,
            warnings: Vec::new(),
            blocked_corner: None,
        };
        if width < MIN_HABITAT_DIMENSION || height < MIN_HABITAT_DIMENSION {
            return plan.reject(format!(
                "Enclose at least {MIN_HABITAT_DIMENSION} tiles across and {MIN_HABITAT_DIMENSION} down"
            ));
        }
        if let Some(habitat) = habitat
            && habitat.tiles == plan.occupied_tiles
        {
            plan.cost_cents = 0;
            plan.service_gate = Some(habitat.service_gate);
            plan.message = format!("Habitat #{} already uses this fence", habitat.id);
            return plan;
        }

        let own = |tile: &Position| habitat.is_some_and(|habitat| habitat.contains(*tile));
        if plan
            .occupied_tiles
            .iter()
            .any(|tile| !own(tile) && self.tile(tile.x, tile.y) != Some(TileKind::Grass))
        {
            return plan.reject("The enclosed area must be clear grass");
        }
        if let Some(owner) = plan.occupied_tiles.iter().find_map(|tile| {
            self.service_gate_owner(*tile)
                .filter(|owner| Some(*owner) != habitat_id)
        }) {
            return plan.reject(gate_access_message(owner));
        }
        let touches_path = plan.occupied_tiles.iter().any(|tile| {
            self.neighbors(*tile).into_iter().any(|neighbor| {
                matches!(
                    self.tile(neighbor.x, neighbor.y),
                    Some(TileKind::Path | TileKind::Entrance)
                )
            })
        });
        if !touches_path {
            return plan.reject("The fence needs at least one path along its outside edge");
        }

        let gate = match habitat {
            Some(habitat)
                if plan.fence_segments.contains(&habitat.service_gate)
                    && self.gate_usable_for(
                        habitat.service_gate,
                        habitat.id,
                        &plan.occupied_tiles,
                    ) =>
            {
                habitat.service_gate
            }
            _ => self.default_service_gate(&plan.fence_segments),
        };
        plan.service_gate = Some(gate);
        if !habitat_id.map_or_else(
            || self.gate_segment_usable(gate),
            |id| self.gate_usable_for(gate, id, &plan.occupied_tiles),
        ) {
            return plan.reject("No fence segment can hold a service gate onto a path or grass");
        }

        let Some(habitat) = habitat else {
            plan.message = "Fence loop can be built here".to_owned();
            return plan;
        };
        if habitat.animal_count() > plan.capacity {
            let message = format!(
                "Habitat #{} keeps {} animals; this fence holds only {}",
                habitat.id,
                habitat.animal_count(),
                plan.capacity
            );
            return plan.reject(message);
        }
        if let Some(keeper) = self
            .keepers
            .iter()
            .find(|keeper| self.tile(keeper.x, keeper.y) == Some(TileKind::Habitat(habitat.id)))
        {
            return plan.reject(format!(
                "Keeper #{} is working inside; reshape once they leave",
                keeper.id
            ));
        }
        if gate != habitat.service_gate {
            plan.warnings.push(format!(
                "The service gate moves to tile {},{} ({:?} side)",
                gate.x, gate.y, gate.side
            ));
        }
        if let Some(species) = &habitat.species {
            let needed = habitat
                .animal_count()
                .saturating_mul(species.welfare_profile().space_per_animal);
            if area < needed && area < habitat.area() {
                plan.warnings.push(format!(
                    "Animals need {needed} tiles; welfare falls with {area}"
                ));
            }
        }
        plan.message = format!(
            "Reshape habitat #{}: {} → {} tiles, fence {}",
            habitat.id,
            habitat.area(),
            area,
            perimeter
        );
        plan
    }

    /// Whether `segment` can hold habitat `habitat_id`'s gate once its fence encloses `tiles`:
    /// the tiles the habitat gives up count as the grass they become.
    pub(crate) fn gate_usable_for(
        &self,
        segment: FenceSegment,
        habitat_id: u32,
        tiles: &[Position],
    ) -> bool {
        self.gate_access_tile(segment)
            .is_some_and(|access| match self.tile(access.x, access.y) {
                Some(TileKind::Grass | TileKind::Path | TileKind::Entrance) => true,
                Some(TileKind::Habitat(id)) => id == habitat_id && !shape::contains(tiles, access),
                _ => false,
            })
    }
}
