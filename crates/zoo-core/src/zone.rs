//! Keeper work zones (#135).
//!
//! A work zone is one map-aligned rectangle; zones never overlap. A habitat belongs to the zone
//! containing its service gate's fence tile, so one gate resolves to zero or one zone. Each
//! keeper has at most one zone; a keeper without one is idle and flagged as needing a zone.
//! The keeper scheduler (#134) only gives a keeper habitat jobs from its own zone. Zone, gate
//! and keeper edits change future eligibility; a job already under way runs to completion.
use super::{ActionResult, GameState, Habitat, Position};
use serde::Serialize;

pub(crate) const MAX_WORK_ZONES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct WorkZone {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A normalized rectangle between two dragged corner tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ZoneBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ZoneBounds {
    fn from_corners(ax: u32, ay: u32, bx: u32, by: u32) -> Self {
        Self {
            x: ax.min(bx),
            y: ay.min(by),
            width: ax.abs_diff(bx) + 1,
            height: ay.abs_diff(by) + 1,
        }
    }

    fn contains(&self, position: Position) -> bool {
        (self.x..self.x + self.width).contains(&position.x)
            && (self.y..self.y + self.height).contains(&position.y)
    }

    fn overlaps(&self, other: &ZoneBounds) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }
}

impl WorkZone {
    pub(crate) fn bounds(&self) -> ZoneBounds {
        ZoneBounds {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

/// The tile a habitat's membership is decided by: the habitat tile its service gate sits on.
fn gate_tile(habitat: &Habitat) -> Position {
    Position {
        x: habitat.service_gate.x,
        y: habitat.service_gate.y,
    }
}

/// A zone create/resize/move preview: the normalized bounds, whether they may be committed,
/// and which habitats' service gates they would include.
#[derive(Serialize)]
pub(crate) struct WorkZoneEvaluation {
    ok: bool,
    message: String,
    bounds: ZoneBounds,
    habitat_ids: Vec<u32>,
}

#[derive(Serialize)]
pub(crate) struct WorkZoneView {
    id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    /// Habitats whose service gate lies inside the zone.
    habitat_ids: Vec<u32>,
    keeper_ids: Vec<u32>,
}

impl GameState {
    /// The zone containing the habitat's service gate, if any.
    pub(crate) fn habitat_zone(&self, habitat: &Habitat) -> Option<u32> {
        self.zone_at(gate_tile(habitat))
    }

    /// The single zone containing a tile; zones never overlap.
    pub(crate) fn zone_at(&self, position: Position) -> Option<u32> {
        self.work_zones
            .iter()
            .find(|zone| zone.bounds().contains(position))
            .map(|zone| zone.id)
    }

    fn zone_habitat_ids(&self, bounds: ZoneBounds) -> Vec<u32> {
        self.habitats
            .iter()
            .filter(|habitat| bounds.contains(gate_tile(habitat)))
            .map(|habitat| habitat.id)
            .collect()
    }

    /// Validates proposed bounds for a new zone (`zone_id` 0) or an existing one.
    fn validate_work_zone(&self, zone_id: u32, bounds: ZoneBounds) -> Result<(), String> {
        if bounds.x + bounds.width > self.width || bounds.y + bounds.height > self.height {
            return Err("Work zones must stay inside the park".to_owned());
        }
        if zone_id == 0 && self.work_zones.len() >= MAX_WORK_ZONES {
            return Err(format!(
                "The park already has the maximum of {MAX_WORK_ZONES} work zones"
            ));
        }
        if let Some(other) = self
            .work_zones
            .iter()
            .find(|zone| zone.id != zone_id && zone.bounds().overlaps(&bounds))
        {
            return Err(format!(
                "Overlaps work zone #{}; work zones may not overlap",
                other.id
            ));
        }
        Ok(())
    }

    pub(crate) fn evaluate_work_zone(
        &self,
        zone_id: u32,
        ax: u32,
        ay: u32,
        bx: u32,
        by: u32,
    ) -> WorkZoneEvaluation {
        let bounds = ZoneBounds::from_corners(ax, ay, bx, by);
        let habitat_ids = self.zone_habitat_ids(bounds);
        let result = if zone_id != 0 && !self.work_zones.iter().any(|zone| zone.id == zone_id) {
            Err("Select a work zone first".to_owned())
        } else {
            self.validate_work_zone(zone_id, bounds)
        };
        let ok = result.is_ok();
        let message = match result {
            Err(message) => message,
            Ok(()) => {
                let size = format!("{}×{} work zone", bounds.width, bounds.height);
                if habitat_ids.is_empty() {
                    format!("{size} · no habitat service gates inside")
                } else {
                    let list: Vec<String> = habitat_ids.iter().map(|id| format!("#{id}")).collect();
                    format!("{size} · service gates of habitats {}", list.join(", "))
                }
            }
        };
        WorkZoneEvaluation {
            ok,
            message,
            bounds,
            habitat_ids,
        }
    }

    pub(crate) fn create_work_zone(&mut self, ax: u32, ay: u32, bx: u32, by: u32) -> ActionResult {
        let bounds = ZoneBounds::from_corners(ax, ay, bx, by);
        if let Err(message) = self.validate_work_zone(0, bounds) {
            return ActionResult::error(message);
        }
        let id = self.next_work_zone_id;
        self.next_work_zone_id = self.next_work_zone_id.saturating_add(1);
        self.work_zones.push(WorkZone {
            id,
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        });
        ActionResult::ok(format!("Work zone #{id} created"))
    }

    /// Resizes or moves a zone; both are a change of its bounds.
    pub(crate) fn set_work_zone_bounds(
        &mut self,
        zone_id: u32,
        ax: u32,
        ay: u32,
        bx: u32,
        by: u32,
    ) -> ActionResult {
        let bounds = ZoneBounds::from_corners(ax, ay, bx, by);
        let Some(index) = self.work_zones.iter().position(|zone| zone.id == zone_id) else {
            return ActionResult::error("Select a work zone first");
        };
        if self.work_zones[index].bounds() == bounds {
            return ActionResult::ok(format!("Work zone #{zone_id} already has these bounds"));
        }
        if let Err(message) = self.validate_work_zone(zone_id, bounds) {
            return ActionResult::error(message);
        }
        let zone = &mut self.work_zones[index];
        zone.x = bounds.x;
        zone.y = bounds.y;
        zone.width = bounds.width;
        zone.height = bounds.height;
        ActionResult::ok(format!("Work zone #{zone_id} updated"))
    }

    /// Deletes a zone; its keepers become unassigned and its habitats uncovered.
    pub(crate) fn delete_work_zone(&mut self, zone_id: u32) -> ActionResult {
        let Some(index) = self.work_zones.iter().position(|zone| zone.id == zone_id) else {
            return ActionResult::error("Select a work zone first");
        };
        self.work_zones.remove(index);
        for keeper in &mut self.keepers {
            if keeper.zone_id == Some(zone_id) {
                keeper.zone_id = None;
            }
        }
        ActionResult::ok(format!("Work zone #{zone_id} deleted"))
    }

    pub(crate) fn assign_keeper_zone(&mut self, keeper_id: u32, zone_id: u32) -> ActionResult {
        if !self.work_zones.iter().any(|zone| zone.id == zone_id) {
            return ActionResult::error("Select a work zone first");
        }
        let Some(keeper) = self
            .keepers
            .iter_mut()
            .find(|keeper| keeper.id == keeper_id)
        else {
            return ActionResult::error(format!("Keeper #{keeper_id} does not exist"));
        };
        if keeper.zone_id == Some(zone_id) {
            return ActionResult::ok(format!(
                "Keeper #{keeper_id} already works in zone #{zone_id}"
            ));
        }
        keeper.zone_id = Some(zone_id);
        ActionResult::ok(format!(
            "Keeper #{keeper_id} assigned to work zone #{zone_id}"
        ))
    }

    /// How many keepers work in a zone, whatever their categories or shifts.
    pub(crate) fn zone_keeper_count(&self, zone_id: u32) -> usize {
        self.keepers
            .iter()
            .filter(|keeper| keeper.zone_id == Some(zone_id))
            .count()
    }

    pub(crate) fn work_zone_views(&self) -> Vec<WorkZoneView> {
        self.work_zones
            .iter()
            .map(|zone| WorkZoneView {
                id: zone.id,
                x: zone.x,
                y: zone.y,
                width: zone.width,
                height: zone.height,
                habitat_ids: self.zone_habitat_ids(zone.bounds()),
                keeper_ids: self
                    .keepers
                    .iter()
                    .filter(|keeper| keeper.zone_id == Some(zone.id))
                    .map(|keeper| keeper.id)
                    .collect(),
            })
            .collect()
    }

    /// Zone invariants: in-park, non-overlapping, and keepers only reference live zones.
    pub(crate) fn work_zone_violation(&self) -> Option<String> {
        for (index, zone) in self.work_zones.iter().enumerate() {
            let bounds = zone.bounds();
            if zone.width == 0
                || zone.height == 0
                || bounds.x + bounds.width > self.width
                || bounds.y + bounds.height > self.height
            {
                return Some(format!("work zone {} lies outside the park", zone.id));
            }
            if let Some(other) = self.work_zones[index + 1..]
                .iter()
                .find(|other| other.bounds().overlaps(&bounds))
            {
                return Some(format!("work zones {} and {} overlap", zone.id, other.id));
            }
        }
        self.keepers
            .iter()
            .find(|keeper| {
                keeper
                    .zone_id
                    .is_some_and(|zone_id| !self.work_zones.iter().any(|zone| zone.id == zone_id))
            })
            .map(|keeper| format!("keeper {} references a deleted work zone", keeper.id))
    }
}

#[cfg(test)]
impl GameState {
    /// Hires a keeper and assigns it to a park-wide zone (created on first use).
    pub(crate) fn hire_zoned_keeper(&mut self) {
        if self.work_zones.is_empty() {
            let (right, bottom) = (self.width - 1, self.height - 1);
            assert!(self.create_work_zone(0, 0, right, bottom).ok);
        }
        assert!(self.hire_keeper().ok);
        let keeper_id = self.keepers.last().expect("hired").id;
        let zone_id = self.work_zones[0].id;
        assert!(self.assign_keeper_zone(keeper_id, zone_id).ok);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HabitatOrientation;
    use crate::care::{CareJobState, KeeperActivity};

    #[test]
    fn zones_are_normalized_in_park_non_overlapping_and_capped() {
        let mut state = GameState::default();
        assert!(state.create_work_zone(4, 4, 0, 0).ok);
        assert_eq!(
            state.work_zones[0].bounds(),
            ZoneBounds {
                x: 0,
                y: 0,
                width: 5,
                height: 5
            }
        );
        let overlap = state.create_work_zone(4, 4, 6, 6);
        assert!(!overlap.ok);
        assert_eq!(
            overlap.message,
            "Overlaps work zone #1; work zones may not overlap"
        );
        let preview = state.evaluate_work_zone(0, 4, 4, 6, 6);
        assert!(!preview.ok);
        assert_eq!(preview.message, overlap.message);
        assert!(state.create_work_zone(5, 0, 5, 0).ok, "1×1 adjacent zone");
        let outside = state.create_work_zone(18, 12, 20, 13);
        assert!(!outside.ok);
        assert_eq!(outside.message, "Work zones must stay inside the park");

        for x in 6..20 {
            for y in 0..3 {
                if state.work_zones.len() < MAX_WORK_ZONES {
                    assert!(state.create_work_zone(x, y, x, y).ok);
                }
            }
        }
        assert_eq!(state.work_zones.len(), MAX_WORK_ZONES);
        assert!(!state.create_work_zone(0, 10, 0, 10).ok);
        // Editing an existing zone is still allowed at the cap.
        assert!(state.set_work_zone_bounds(1, 0, 5, 4, 9).ok);
        assert!(state.invariant_violation().is_none());
    }

    #[test]
    fn a_habitat_follows_its_service_gate_across_a_zone_boundary() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let id = state.habitats[0].id;
        // The gate sits on (3, 8). Zone #1 covers only the gate's column; zone #2 the rest.
        assert!(state.create_work_zone(3, 8, 3, 12).ok);
        assert!(state.create_work_zone(4, 8, 10, 12).ok);
        assert_eq!(state.habitat_zone(&state.habitats[0]), Some(1));
        let preview = state.evaluate_work_zone(0, 0, 0, 2, 2);
        assert!(preview.habitat_ids.is_empty());
        assert_eq!(
            state.evaluate_work_zone(2, 3, 8, 10, 12).habitat_ids,
            vec![id]
        );

        let gate_preview = state.evaluate_service_gate(id, 6, 9);
        assert_eq!(gate_preview.zone_id(), Some(2));
        assert!(state.set_service_gate(id, 6, 9, "east").ok);
        assert_eq!(state.habitat_zone(&state.habitats[0]), Some(2));
        let views = state.work_zone_views();
        assert!(views[0].habitat_ids.is_empty());
        assert_eq!(views[1].habitat_ids, vec![id]);

        assert!(state.delete_work_zone(2).ok);
        assert_eq!(state.habitat_zone(&state.habitats[0]), None);
        assert!(!state.delete_work_zone(2).ok);
    }

    #[test]
    fn adoption_needs_a_staffed_zone_and_unassigned_keepers_stay_idle() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let id = state.habitats[0].id;
        assert!(state.hire_keeper().ok);
        assert_eq!(
            state.keeper_view(&state.keepers[0]).status,
            "Idle · needs a work zone"
        );

        let uncovered = state.adopt(id, "capybara");
        assert!(!uncovered.ok);
        assert!(uncovered.message.contains("outside every work zone"));
        assert!(state.create_work_zone(0, 0, 9, 13).ok);
        let unstaffed = state.adopt(id, "capybara");
        assert!(!unstaffed.ok);
        assert!(unstaffed.message.contains("No keeper works in zone #1"));

        assert!(state.assign_keeper_zone(1, 1).ok);
        assert!(state.assign_keeper_zone(1, 1).message.contains("already"));
        assert!(!state.assign_keeper_zone(1, 9).ok);
        assert!(!state.assign_keeper_zone(9, 1).ok);
        assert!(state.adopt(id, "capybara").ok);

        // Deleting a zone unassigns its keepers without blocking.
        assert!(state.create_work_zone(10, 0, 19, 13).ok);
        assert!(state.assign_keeper_zone(1, 2).ok);
        assert!(state.delete_work_zone(2).ok);
        assert_eq!(state.keepers[0].zone_id, None);
        assert!(state.invariant_violation().is_none());
    }

    #[test]
    fn a_running_job_completes_after_its_zone_is_deleted() {
        let mut state = GameState::default();
        assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
        let id = state.habitats[0].id;
        assert!(state.buy_animal_feed().ok);
        state.hire_zoned_keeper();
        assert!(state.adopt(id, "capybara").ok);
        state.habitats[0].food = 10;
        let job_id = (0..24 * 60)
            .find_map(|_| {
                state.tick(1);
                match state.keepers[0].activity {
                    KeeperActivity::ToGate { job_id } => Some(job_id),
                    _ => None,
                }
            })
            .expect("a care job is dispatched");

        let deleted = state.delete_work_zone(1);
        assert!(deleted.ok);
        for _ in 0..120 {
            state.tick(1);
        }
        let job = state.care_jobs.iter().find(|job| job.id == job_id).unwrap();
        assert_eq!(job.state, CareJobState::Completed);
        assert!(state.habitats[0].food > 10, "the delivery refilled food");
        assert!(state.keeper_view(&state.keepers[0]).needs_zone);
    }
}
