//! Transactional construction (#92).
//!
//! Every construction gesture is one all-or-nothing transaction. A Rust-owned plan previews the
//! exact footprint, price, affected objects and rejection reason; committing plans again
//! against the current park, so a stale preview is never permission to spend or build.
//!
//! Committed transactions enter a bounded undo history that reverses construction only, never
//! elapsed simulation, births, sales, wages or other world history:
//!
//! - At most [`HISTORY_LIMIT`] transactions are kept; the oldest falls off.
//! - Undo applies a validated inverse and refunds exactly what the transaction spent; redo
//!   validates again, charges the same price and restores the same stable IDs, so undo→redo
//!   cycles neither mint money nor consume IDs.
//! - Undo is refused once the construction has been used or changed (guests or staff standing
//!   on a path, litter, sales, maintenance, adopted animals, shelters, care work). Demolish it
//!   at the normal price instead.
//! - Demolition is free and refunds nothing; it enters the history only when it destroys
//!   nothing irreversible (litter, animals, departing guests, open maintenance or care work).
//! - History lasts until the in-game day closes. A refund reduces that same day's construction
//!   expense, so the ledger never shows a refund for spending it does not contain.
//! - History is session state: saves do not carry it and a load starts with none. Undo and redo
//!   are recorded commands, so session replay reproduces them exactly.
use super::{
    ActionResult, Concession, ConcessionKind, ExpenseCategory, GameState, Habitat, PATH_COST,
    PlacementEvaluation, Position, TileKind, content, gate::gate_access_message,
};
use serde::Serialize;
use std::collections::HashSet;

/// Undoable transactions kept per session.
pub(crate) const HISTORY_LIMIT: usize = 16;
/// The largest path stroke one gesture may build.
pub(crate) const MAX_STROKE_TILES: usize = 256;

#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
}

impl History {
    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn record(&mut self, transaction: Transaction) {
        self.redo.clear();
        self.undo.push(transaction);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
    }
}

#[derive(Clone, Debug)]
struct Transaction {
    label: String,
    cost_cents: i64,
    change: Change,
}

#[derive(Clone, Debug)]
enum Change {
    BuildPaths(Vec<Position>),
    BuildConcession {
        id: u32,
        x: u32,
        y: u32,
        key: String,
    },
    BuildHabitat {
        id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    RemovePath(Position),
    RemoveConcession(Box<Concession>),
    RemoveHabitat(Box<Habitat>),
}

/// What committing a construction action would do, computed by the same plan the commit uses.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ConstructionPreview {
    ok: bool,
    /// False when committing would leave the park as it is (an existing path, an empty tile).
    changes: bool,
    message: String,
    action: &'static str,
    cost_cents: i64,
    /// The tiles the action builds on or clears.
    tiles: Vec<Position>,
    /// The tiles that make a rejected action fail.
    blocked_tiles: Vec<Position>,
    affected: Vec<Affected>,
    /// Whether the committed action enters the undo history.
    undoable: bool,
}

#[derive(Clone, Debug, Serialize)]
struct Affected {
    kind: &'static str,
    id: Option<u32>,
    effect: String,
}

impl ConstructionPreview {
    fn rejected(
        action: &'static str,
        message: impl Into<String>,
        tiles: Vec<Position>,
        blocked_tiles: Vec<Position>,
    ) -> Self {
        Self {
            ok: false,
            changes: false,
            message: message.into(),
            action,
            cost_cents: 0,
            tiles,
            blocked_tiles,
            affected: Vec::new(),
            undoable: false,
        }
    }

    fn unchanged(action: &'static str, message: impl Into<String>) -> Self {
        Self {
            ok: true,
            ..Self::rejected(action, message, Vec::new(), Vec::new())
        }
    }

    fn planned(
        action: &'static str,
        message: impl Into<String>,
        cost_cents: i64,
        tiles: Vec<Position>,
    ) -> Self {
        Self {
            ok: true,
            changes: true,
            cost_cents,
            undoable: true,
            ..Self::rejected(action, message, tiles, Vec::new())
        }
    }
}

#[derive(Serialize)]
pub(crate) struct HistoryView {
    limit: usize,
    undo_depth: usize,
    redo_depth: usize,
    undo: Option<StepView>,
    redo: Option<StepView>,
}

#[derive(Serialize)]
struct StepView {
    label: String,
    cost_cents: i64,
    /// Whether the step can run against the current park, and why not.
    ok: bool,
    message: String,
}

#[derive(Clone, Copy, Debug)]
enum Target {
    Path(Position),
    Concession(u32),
    Habitat(u32),
}

pub(crate) fn dollars(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    if cents.is_multiple_of(100) {
        format!("{sign}${}", cents / 100)
    } else {
        format!("{sign}${}.{:02}", cents / 100, cents % 100)
    }
}

/// The 4-connected tiles a stroke through `points` (flat `x, y` pairs) covers, in order and
/// without repeats. Consecutive points are joined, so a fast drag leaves no gaps.
fn stroke_tiles(points: &[u32]) -> Result<Vec<Position>, String> {
    if points.is_empty() || !points.len().is_multiple_of(2) {
        return Err("A path stroke needs whole x, y tile pairs".to_owned());
    }
    let too_long = || format!("A path stroke may cover at most {MAX_STROKE_TILES} tiles");
    let mut tiles = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |tile: Position, tiles: &mut Vec<Position>| {
        if seen.insert((tile.x, tile.y)) {
            tiles.push(tile);
        }
        tiles.len() <= MAX_STROKE_TILES
    };
    let mut previous: Option<Position> = None;
    for pair in points.as_chunks::<2>().0 {
        let to = Position {
            x: pair[0],
            y: pair[1],
        };
        let Some(from) = previous else {
            push(to, &mut tiles);
            previous = Some(to);
            continue;
        };
        let dx = i64::from(to.x) - i64::from(from.x);
        let dy = i64::from(to.y) - i64::from(from.y);
        let (span_x, span_y) = (dx.abs(), dy.abs());
        if span_x + span_y > MAX_STROKE_TILES as i64 {
            return Err(too_long());
        }
        let (mut x, mut y) = (i64::from(from.x), i64::from(from.y));
        let (mut step_x, mut step_y) = (0, 0);
        while step_x < span_x || step_y < span_y {
            // Step along whichever axis lags the straight line between the points.
            if (1 + 2 * step_x) * span_y < (1 + 2 * step_y) * span_x {
                x += dx.signum();
                step_x += 1;
            } else {
                y += dy.signum();
                step_y += 1;
            }
            let tile = Position {
                x: x as u32,
                y: y as u32,
            };
            if !push(tile, &mut tiles) {
                return Err(too_long());
            }
        }
        previous = Some(to);
    }
    if tiles.len() > MAX_STROKE_TILES {
        return Err(too_long());
    }
    Ok(tiles)
}

impl GameState {
    pub(crate) fn construction_view(&self) -> HistoryView {
        let step = |transaction: Option<&Transaction>,
                    check: &dyn Fn(&Transaction) -> Result<(), String>| {
            transaction.map(|transaction| {
                let checked = check(transaction);
                StepView {
                    label: transaction.label.clone(),
                    cost_cents: transaction.cost_cents,
                    ok: checked.is_ok(),
                    message: checked.err().unwrap_or_default(),
                }
            })
        };
        HistoryView {
            limit: HISTORY_LIMIT,
            undo_depth: self.construction.undo.len(),
            redo_depth: self.construction.redo.len(),
            undo: step(self.construction.undo.last(), &|t| self.check_undo(t)),
            redo: step(self.construction.redo.last(), &|t| self.check_redo(t)),
        }
    }

    fn record_construction(&mut self, label: String, cost_cents: i64, change: Change) {
        self.construction.record(Transaction {
            label,
            cost_cents,
            change,
        });
    }

    /// Who stands on a path tile and would be stranded by removing it.
    fn path_occupant(&self, tile: Position) -> Option<&'static str> {
        if self.guests.iter().any(|g| g.x == tile.x && g.y == tile.y) {
            return Some("A guest is standing on that path tile");
        }
        if self.janitors.iter().any(|j| j.x == tile.x && j.y == tile.y) {
            return Some("A janitor is standing on that path tile");
        }
        if self
            .mechanics
            .iter()
            .any(|m| m.x == tile.x && m.y == tile.y)
        {
            return Some("A mechanic is standing on that path tile");
        }
        None
    }

    // --- Paths -----------------------------------------------------------------------------

    pub(crate) fn evaluate_path_stroke(&self, points: &[u32]) -> ConstructionPreview {
        self.plan_path_stroke(points)
    }

    fn plan_path_stroke(&self, points: &[u32]) -> ConstructionPreview {
        let stroke = match stroke_tiles(points) {
            Ok(stroke) => stroke,
            Err(message) => {
                return ConstructionPreview::rejected("path", message, Vec::new(), Vec::new());
            }
        };
        let mut new_tiles = Vec::new();
        let mut blocked = Vec::new();
        let mut reason = None;
        for &tile in &stroke {
            let problem = match self.tile(tile.x, tile.y) {
                None => Some("The stroke leaves the park"),
                Some(TileKind::Path | TileKind::Entrance) => None,
                Some(TileKind::Grass) => {
                    new_tiles.push(tile);
                    None
                }
                Some(TileKind::Habitat(_)) => Some("A habitat blocks the stroke"),
                Some(TileKind::Concession(_)) => Some("A concession stand blocks the stroke"),
                Some(TileKind::AnimalCareDepot) => {
                    Some("The central animal-care depot blocks the stroke")
                }
            };
            if let Some(problem) = problem {
                blocked.push(tile);
                reason.get_or_insert(problem);
            }
        }
        if let Some(reason) = reason {
            return ConstructionPreview::rejected(
                "path",
                format!("{reason} · nothing is built"),
                new_tiles,
                blocked,
            );
        }
        if new_tiles.is_empty() {
            return ConstructionPreview::unchanged("path", "Path already exists");
        }
        let cost = PATH_COST.saturating_mul(new_tiles.len() as i64);
        if self.cash_cents < cost {
            return ConstructionPreview::rejected(
                "path",
                format!("Not enough cash: this stroke costs {}", dollars(cost)),
                new_tiles,
                Vec::new(),
            );
        }
        let count = new_tiles.len();
        ConstructionPreview::planned(
            "path",
            format!(
                "{count} new path tile{} · {}",
                if count == 1 { "" } else { "s" },
                dollars(cost)
            ),
            cost,
            new_tiles,
        )
    }

    pub(crate) fn build_path_stroke(&mut self, points: &[u32]) -> ActionResult {
        let plan = self.plan_path_stroke(points);
        if !plan.ok {
            return ActionResult::error(plan.message);
        }
        if !plan.changes {
            return ActionResult::ok(plan.message);
        }
        if let Err(message) = self.spend(plan.cost_cents, ExpenseCategory::Construction) {
            return ActionResult::error(message);
        }
        self.lay_paths(&plan.tiles);
        let count = plan.tiles.len();
        let label = format!("{count} path tile{}", if count == 1 { "" } else { "s" });
        self.record_construction(
            label,
            plan.cost_cents,
            Change::BuildPaths(plan.tiles.clone()),
        );
        ActionResult::ok(format!(
            "Path built · {count} new tile{} · {}",
            if count == 1 { "" } else { "s" },
            dollars(plan.cost_cents)
        ))
    }

    fn lay_paths(&mut self, tiles: &[Position]) {
        for tile in tiles {
            self.set_tile(tile.x, tile.y, TileKind::Path);
        }
        self.refresh_guest_routes();
    }

    /// A one-tile stroke, for focused tests.
    #[cfg(test)]
    pub(crate) fn place_path(&mut self, x: u32, y: u32) -> ActionResult {
        self.build_path_stroke(&[x, y])
    }

    // --- Concessions -----------------------------------------------------------------------

    pub(crate) fn evaluate_concession(
        &self,
        x: u32,
        y: u32,
        kind_name: &str,
    ) -> ConstructionPreview {
        let tiles = vec![Position { x, y }];
        let reject = |message: String| {
            ConstructionPreview::rejected("facility", message, Vec::new(), tiles.clone())
        };
        let Some(definition) = self
            .catalog
            .facilities
            .iter()
            .find(|facility| facility.key == kind_name)
        else {
            return reject("Unknown concession type".to_owned());
        };
        if ConcessionKind::parse(&definition.service).is_none() {
            return reject("Unknown concession type".to_owned());
        }
        match self.tile(x, y) {
            None => return reject("That tile is outside the park".to_owned()),
            Some(TileKind::Concession(id)) => {
                let Some(existing) = self.concessions.iter().find(|stand| stand.id == id) else {
                    return reject("The concession tile is inconsistent".to_owned());
                };
                if existing.definition.key == definition.key {
                    return ConstructionPreview::unchanged(
                        "facility",
                        format!("{} already stands here", definition.label),
                    );
                }
                return reject(
                    "A different concession stand already occupies that tile".to_owned(),
                );
            }
            Some(TileKind::Path | TileKind::Entrance) => {
                return reject("Build the stand on grass beside the path".to_owned());
            }
            Some(TileKind::Habitat(_)) => return reject("A habitat occupies that tile".to_owned()),
            Some(TileKind::AnimalCareDepot) => {
                return reject("The central animal-care depot occupies that tile".to_owned());
            }
            Some(TileKind::Grass) => {}
        }
        let position = Position { x, y };
        if let Some(habitat_id) = self.service_gate_owner(position) {
            return reject(gate_access_message(habitat_id));
        }
        if !self.touches_walkable(position) {
            return reject("Concession stands must touch a guest path".to_owned());
        }
        let cost = definition.build_cost_cents;
        if self.cash_cents < cost {
            return reject(format!(
                "Not enough cash: {} costs {}",
                definition.label,
                dollars(cost)
            ));
        }
        ConstructionPreview::planned(
            "facility",
            format!("{} · {}", definition.label, dollars(cost)),
            cost,
            tiles,
        )
    }

    fn touches_walkable(&self, position: Position) -> bool {
        self.neighbors(position)
            .into_iter()
            .any(|neighbor| self.is_walkable(neighbor))
    }

    pub(crate) fn place_concession(&mut self, x: u32, y: u32, kind_name: &str) -> ActionResult {
        let plan = self.evaluate_concession(x, y, kind_name);
        if !plan.ok {
            return ActionResult::error(plan.message);
        }
        if !plan.changes {
            return ActionResult::ok(plan.message);
        }
        if let Err(message) = self.spend(plan.cost_cents, ExpenseCategory::Construction) {
            return ActionResult::error(message);
        }
        let id = self.next_concession_id;
        self.next_concession_id += 1;
        let label = self.insert_concession(id, x, y, kind_name);
        self.record_construction(
            format!("{label} #{id}"),
            plan.cost_cents,
            Change::BuildConcession {
                id,
                x,
                y,
                key: kind_name.to_owned(),
            },
        );
        ActionResult::ok(format!("{label} #{id} built beside the path"))
    }

    /// Places a planned stand under `id`; returns its label.
    fn insert_concession(&mut self, id: u32, x: u32, y: u32, key: &str) -> String {
        let definition = self
            .catalog
            .facilities
            .iter()
            .find(|facility| facility.key == key)
            .cloned()
            .expect("planned concession definition");
        let kind = ConcessionKind::parse(&definition.service).expect("planned concession kind");
        let label = definition.label.clone();
        self.restore_concession(Concession {
            id,
            x,
            y,
            kind,
            definition,
            sales_today: 0,
            total_sales: 0,
            total_revenue_cents: 0,
            condition: 100,
        });
        label
    }

    fn restore_concession(&mut self, stand: Concession) {
        self.set_tile(stand.x, stand.y, TileKind::Concession(stand.id));
        let at = self
            .concessions
            .partition_point(|other| other.id < stand.id);
        self.concessions.insert(at, stand);
    }

    // --- Habitats --------------------------------------------------------------------------

    pub(crate) fn place_habitat_rect(
        &mut self,
        ax: u32,
        ay: u32,
        bx: u32,
        by: u32,
    ) -> ActionResult {
        let (x, y, width, height) = super::normalized_rect(ax, ay, bx, by);
        if let Some(existing) = self.habitats.iter().find(|habitat| {
            habitat.x == x && habitat.y == y && habitat.width == width && habitat.height == height
        }) {
            return ActionResult::ok(format!("Habitat #{} already uses this fence", existing.id));
        }
        let evaluation = self.evaluate_habitat_rect(ax, ay, bx, by);
        if !evaluation.ok {
            return ActionResult::error(evaluation.message);
        }
        if let Err(message) = self.spend(evaluation.cost_cents, ExpenseCategory::Construction) {
            return ActionResult::error(message);
        }
        let id = self.next_habitat_id;
        self.next_habitat_id += 1;
        self.insert_habitat(id, &evaluation);
        self.record_construction(
            format!("habitat #{id}"),
            evaluation.cost_cents,
            Change::BuildHabitat {
                id,
                x: evaluation.x,
                y: evaluation.y,
                width: evaluation.width,
                height: evaluation.height,
            },
        );
        ActionResult::ok(format!(
            "Habitat #{id} fenced · {}×{} · {} tiles",
            evaluation.width,
            evaluation.height,
            evaluation.width.saturating_mul(evaluation.height)
        ))
    }

    fn insert_habitat(&mut self, id: u32, evaluation: &PlacementEvaluation) {
        let service_gate = self.default_service_gate(
            evaluation.x,
            evaluation.y,
            evaluation.width,
            evaluation.height,
        );
        self.restore_habitat(Habitat {
            id,
            x: evaluation.x,
            y: evaluation.y,
            width: evaluation.width,
            height: evaluation.height,
            orientation: evaluation.orientation,
            species: None,
            animals: 0,
            welfare: 100,
            food: 100,
            water: 100,
            cleanliness: 100,
            has_shelter: false,
            care_schedule: None,
            service_gate,
        });
    }

    fn restore_habitat(&mut self, habitat: Habitat) {
        for tile in habitat_tiles(&habitat) {
            self.set_tile(tile.x, tile.y, TileKind::Habitat(habitat.id));
        }
        let at = self.habitats.partition_point(|other| other.id < habitat.id);
        self.habitats.insert(at, habitat);
    }

    // --- Demolition ------------------------------------------------------------------------

    pub(crate) fn evaluate_demolition(&self, x: u32, y: u32) -> ConstructionPreview {
        self.plan_demolition(x, y).0
    }

    fn plan_demolition(&self, x: u32, y: u32) -> (ConstructionPreview, Option<Target>) {
        let position = Position { x, y };
        let reject = |message: &str| {
            (
                ConstructionPreview::rejected("demolish", message, Vec::new(), vec![position]),
                None,
            )
        };
        match self.tile(x, y) {
            None => reject("That tile is outside the park"),
            Some(TileKind::Grass) => (
                ConstructionPreview::unchanged("demolish", "Nothing to demolish"),
                None,
            ),
            Some(TileKind::Entrance) => reject("The park entrance cannot be demolished"),
            Some(TileKind::AnimalCareDepot) => {
                reject("The central animal-care depot cannot be demolished")
            }
            Some(TileKind::Path) => {
                if let Some(occupant) = self.path_occupant(position) {
                    return reject(occupant);
                }
                let litter = self
                    .litter
                    .iter()
                    .filter(|task| task.x == x && task.y == y)
                    .count();
                let mut plan =
                    ConstructionPreview::planned("demolish", "Remove path tile", 0, vec![position]);
                if litter > 0 {
                    plan.affected.push(Affected {
                        kind: "litter",
                        id: None,
                        effect: format!("{litter} litter cleared for good"),
                    });
                    plan.undoable = false;
                }
                (plan, Some(Target::Path(position)))
            }
            Some(TileKind::Concession(id)) => {
                let Some(stand) = self.concessions.iter().find(|stand| stand.id == id) else {
                    return reject("The concession tile is inconsistent");
                };
                let mut plan = ConstructionPreview::planned(
                    "demolish",
                    format!("Remove {} #{id}", stand.definition.label),
                    0,
                    vec![position],
                );
                plan.affected.push(Affected {
                    kind: "concession",
                    id: Some(id),
                    effect: "removed".to_owned(),
                });
                if self.maintenance.iter().any(|task| task.concession_id == id) {
                    plan.affected.push(Affected {
                        kind: "maintenance",
                        id: None,
                        effect: "open maintenance cancelled".to_owned(),
                    });
                    plan.undoable = false;
                }
                (plan, Some(Target::Concession(id)))
            }
            Some(TileKind::Habitat(id)) => {
                let Some(habitat) = self.habitats.iter().find(|habitat| habitat.id == id) else {
                    return reject("The habitat tile is inconsistent");
                };
                let mut plan = ConstructionPreview::planned(
                    "demolish",
                    format!("Remove habitat #{id}"),
                    0,
                    habitat_tiles(habitat),
                );
                plan.affected.push(Affected {
                    kind: "habitat",
                    id: Some(id),
                    effect: "fence and enclosure removed".to_owned(),
                });
                if habitat.animals > 0 {
                    plan.affected.push(Affected {
                        kind: "animals",
                        id: None,
                        effect: format!("{} animals leave the park", habitat.animals),
                    });
                }
                let departing = self
                    .guests
                    .iter()
                    .filter(|guest| guest.target_habitat == id)
                    .count();
                if departing > 0 {
                    plan.affected.push(Affected {
                        kind: "guests",
                        id: None,
                        effect: format!("{departing} guests heading there leave"),
                    });
                }
                let care_work = self.care_jobs.iter().any(|job| job.habitat_id == id);
                if care_work {
                    plan.affected.push(Affected {
                        kind: "care",
                        id: None,
                        effect: "scheduled care work cancelled".to_owned(),
                    });
                }
                plan.undoable = habitat.species.is_none()
                    && habitat.animals == 0
                    && departing == 0
                    && !care_work;
                (plan, Some(Target::Habitat(id)))
            }
        }
    }

    pub(crate) fn bulldoze(&mut self, x: u32, y: u32) -> ActionResult {
        let (plan, target) = self.plan_demolition(x, y);
        if !plan.ok {
            return ActionResult::error(plan.message);
        }
        let Some(target) = target else {
            return ActionResult::ok(plan.message);
        };
        let (label, change, message) = match target {
            Target::Path(position) => {
                self.remove_path(position);
                (
                    "path tile removal".to_owned(),
                    Change::RemovePath(position),
                    "Path removed".to_owned(),
                )
            }
            Target::Concession(id) => {
                let stand = self.remove_concession(id);
                (
                    format!("{} #{id} removal", stand.definition.label),
                    Change::RemoveConcession(Box::new(stand)),
                    format!("Concession stand #{id} removed"),
                )
            }
            Target::Habitat(id) => {
                let habitat = self.remove_habitat(id);
                (
                    format!("habitat #{id} removal"),
                    Change::RemoveHabitat(Box::new(habitat)),
                    format!("Habitat #{id} removed and its keeper released"),
                )
            }
        };
        if plan.undoable {
            self.record_construction(label, 0, change);
        }
        ActionResult::ok(message)
    }

    fn remove_path(&mut self, position: Position) {
        let removed_litter_ids: Vec<u32> = self
            .litter
            .iter()
            .filter(|task| task.x == position.x && task.y == position.y)
            .map(|task| task.id)
            .collect();
        self.litter
            .retain(|task| task.x != position.x || task.y != position.y);
        for janitor in &mut self.janitors {
            if janitor
                .target_litter_id
                .is_some_and(|id| removed_litter_ids.contains(&id))
            {
                janitor.target_litter_id = None;
            }
        }
        self.set_tile(position.x, position.y, TileKind::Grass);
        self.refresh_guest_routes();
        self.release_unreachable_mechanic_assignments();
    }

    fn remove_concession(&mut self, id: u32) -> Concession {
        let index = self
            .concessions
            .iter()
            .position(|stand| stand.id == id)
            .expect("planned concession exists");
        let stand = self.concessions.remove(index);
        self.set_tile(stand.x, stand.y, TileKind::Grass);
        let removed_task_ids: Vec<u32> = self
            .maintenance
            .iter()
            .filter(|task| task.concession_id == id)
            .map(|task| task.id)
            .collect();
        self.maintenance.retain(|task| task.concession_id != id);
        for mechanic in &mut self.mechanics {
            if mechanic
                .target_maintenance_id
                .is_some_and(|task_id| removed_task_ids.contains(&task_id))
            {
                mechanic.target_maintenance_id = None;
            }
        }
        stand
    }

    fn remove_habitat(&mut self, id: u32) -> Habitat {
        self.release_habitat_keepers(id);
        let index = self
            .habitats
            .iter()
            .position(|habitat| habitat.id == id)
            .expect("planned habitat exists");
        let habitat = self.habitats.remove(index);
        for tile in habitat_tiles(&habitat) {
            self.set_tile(tile.x, tile.y, TileKind::Grass);
        }
        let departing: Vec<u32> = self
            .guests
            .iter()
            .filter(|guest| guest.target_habitat == id)
            .map(|guest| guest.id)
            .collect();
        for guest_id in departing {
            self.emit("guest_left", Some(guest_id));
        }
        self.guests.retain(|guest| guest.target_habitat != id);
        habitat
    }

    // --- Undo / redo -----------------------------------------------------------------------

    fn check_refund(&self, cents: i64) -> Result<(), String> {
        if self.finance_today.construction_expense_cents < cents {
            return Err("today's construction ledger no longer holds that spending".to_owned());
        }
        self.cash_cents
            .checked_add(cents)
            .filter(|cash| (-content::MAX_CENTS..=content::MAX_CENTS).contains(cash))
            .map(|_| ())
            .ok_or_else(|| "the refund exceeds the exact financial range".to_owned())
    }

    fn refund_construction(&mut self, cents: i64) {
        self.finance_today.construction_expense_cents -= cents;
        self.cash_cents += cents;
    }

    fn check_undo(&self, transaction: &Transaction) -> Result<(), String> {
        let used = |reason: String| Err(format!("{reason}; demolish it instead"));
        match &transaction.change {
            Change::BuildPaths(tiles) => {
                for tile in tiles {
                    if self.tile(tile.x, tile.y) != Some(TileKind::Path) {
                        return Err("the park changed where that path was built".to_owned());
                    }
                    if let Some(occupant) = self.path_occupant(*tile) {
                        return used(occupant.to_owned());
                    }
                    if self
                        .litter
                        .iter()
                        .any(|task| task.x == tile.x && task.y == tile.y)
                    {
                        return used("guests have littered on that path".to_owned());
                    }
                }
            }
            Change::BuildConcession { id, .. } => {
                let Some(stand) = self.concessions.iter().find(|stand| stand.id == *id) else {
                    return Err(format!("stand #{id} no longer exists"));
                };
                if stand.total_sales > 0 {
                    return used(format!("stand #{id} has served guests"));
                }
                if self
                    .maintenance
                    .iter()
                    .any(|task| task.concession_id == *id)
                {
                    return used(format!("stand #{id} needs maintenance"));
                }
            }
            Change::BuildHabitat { id, .. } => {
                let Some(habitat) = self.habitats.iter().find(|habitat| habitat.id == *id) else {
                    return Err(format!("habitat #{id} no longer exists"));
                };
                if habitat.species.is_some() || habitat.animals > 0 {
                    return used(format!("habitat #{id} has animals"));
                }
                if habitat.has_shelter {
                    return used(format!("habitat #{id} has a shelter"));
                }
                if habitat.care_schedule.is_some()
                    || self.care_jobs.iter().any(|job| job.habitat_id == *id)
                {
                    return used(format!("habitat #{id} has care work"));
                }
                if self.guests.iter().any(|guest| guest.target_habitat == *id) {
                    return used(format!("guests are visiting habitat #{id}"));
                }
            }
            Change::RemovePath(tile) => {
                if self.tile(tile.x, tile.y) != Some(TileKind::Grass) {
                    return Err("something else now stands on that tile".to_owned());
                }
            }
            Change::RemoveConcession(stand) => {
                let position = Position {
                    x: stand.x,
                    y: stand.y,
                };
                if self.tile(stand.x, stand.y) != Some(TileKind::Grass) {
                    return Err("something else now stands on that tile".to_owned());
                }
                if let Some(habitat_id) = self.service_gate_owner(position) {
                    return Err(gate_access_message(habitat_id));
                }
                if !self.touches_walkable(position) {
                    return Err("the stand would no longer touch a guest path".to_owned());
                }
            }
            Change::RemoveHabitat(habitat) => {
                for tile in habitat_tiles(habitat) {
                    if self.tile(tile.x, tile.y) != Some(TileKind::Grass) {
                        return Err("something else now stands inside that fence".to_owned());
                    }
                    if let Some(owner) = self.service_gate_owner(tile) {
                        return Err(gate_access_message(owner));
                    }
                }
                if !self.gate_segment_usable(habitat.service_gate) {
                    return Err("its service gate would open onto a building".to_owned());
                }
            }
        }
        self.check_refund(transaction.cost_cents)
    }

    fn apply_undo(&mut self, transaction: &Transaction) {
        match &transaction.change {
            Change::BuildPaths(tiles) => {
                for tile in tiles {
                    self.set_tile(tile.x, tile.y, TileKind::Grass);
                }
                self.refresh_guest_routes();
                self.release_unreachable_mechanic_assignments();
            }
            Change::BuildConcession { id, .. } => {
                self.remove_concession(*id);
            }
            Change::BuildHabitat { id, .. } => {
                self.remove_habitat(*id);
            }
            Change::RemovePath(tile) => self.lay_paths(&[*tile]),
            Change::RemoveConcession(stand) => self.restore_concession(stand.as_ref().clone()),
            Change::RemoveHabitat(habitat) => self.restore_habitat(habitat.as_ref().clone()),
        }
        self.refund_construction(transaction.cost_cents);
    }

    fn check_redo(&self, transaction: &Transaction) -> Result<(), String> {
        let cost = match &transaction.change {
            Change::BuildPaths(tiles) => {
                if tiles
                    .iter()
                    .any(|tile| self.tile(tile.x, tile.y) != Some(TileKind::Grass))
                {
                    return Err("those tiles are no longer clear grass".to_owned());
                }
                PATH_COST.saturating_mul(tiles.len() as i64)
            }
            Change::BuildConcession { x, y, key, .. } => {
                let plan = self.evaluate_concession(*x, *y, key);
                if !plan.ok || !plan.changes {
                    return Err(plan.message);
                }
                plan.cost_cents
            }
            Change::BuildHabitat {
                x,
                y,
                width,
                height,
                ..
            } => {
                let plan = self.evaluate_habitat_rect(*x, *y, x + width - 1, y + height - 1);
                if !plan.ok {
                    return Err(plan.message);
                }
                plan.cost_cents
            }
            Change::RemovePath(tile) => {
                self.check_redo_demolition(tile.x, tile.y, |target| {
                    matches!(target, Target::Path(_))
                })?;
                0
            }
            Change::RemoveConcession(stand) => {
                let id = stand.id;
                self.check_redo_demolition(
                    stand.x,
                    stand.y,
                    |target| matches!(target, Target::Concession(other) if other == id),
                )?;
                0
            }
            Change::RemoveHabitat(habitat) => {
                let id = habitat.id;
                self.check_redo_demolition(
                    habitat.x,
                    habitat.y,
                    |target| matches!(target, Target::Habitat(other) if other == id),
                )?;
                0
            }
        };
        if cost != transaction.cost_cents {
            return Err(format!(
                "its price changed from {} to {}",
                dollars(transaction.cost_cents),
                dollars(cost)
            ));
        }
        if self.cash_cents < cost {
            return Err(format!("not enough cash: it costs {}", dollars(cost)));
        }
        Ok(())
    }

    fn check_redo_demolition(
        &self,
        x: u32,
        y: u32,
        expected: impl Fn(Target) -> bool,
    ) -> Result<(), String> {
        let (plan, target) = self.plan_demolition(x, y);
        if !plan.ok {
            return Err(plan.message);
        }
        if !target.is_some_and(expected) {
            return Err("the park changed where that demolition happened".to_owned());
        }
        if !plan.undoable {
            return Err("demolishing it now would destroy more than before".to_owned());
        }
        Ok(())
    }

    fn apply_redo(&mut self, transaction: &Transaction) -> Result<(), &'static str> {
        self.spend(transaction.cost_cents, ExpenseCategory::Construction)?;
        match &transaction.change {
            Change::BuildPaths(tiles) => self.lay_paths(tiles),
            Change::BuildConcession { id, x, y, key } => {
                self.insert_concession(*id, *x, *y, key);
            }
            Change::BuildHabitat {
                id,
                x,
                y,
                width,
                height,
            } => {
                let plan = self.evaluate_habitat_rect(*x, *y, x + width - 1, y + height - 1);
                self.insert_habitat(*id, &plan);
            }
            Change::RemovePath(tile) => self.remove_path(*tile),
            Change::RemoveConcession(stand) => {
                self.remove_concession(stand.id);
            }
            Change::RemoveHabitat(habitat) => {
                self.remove_habitat(habitat.id);
            }
        }
        Ok(())
    }

    pub(crate) fn undo_construction(&mut self) -> ActionResult {
        let Some(transaction) = self.construction.undo.last().cloned() else {
            return ActionResult::error("Nothing to undo");
        };
        if let Err(reason) = self.check_undo(&transaction) {
            return ActionResult::error(format!("Cannot undo {}: {reason}", transaction.label));
        }
        self.apply_undo(&transaction);
        self.construction.undo.pop();
        let message = if transaction.cost_cents > 0 {
            format!(
                "Undid {} · refunded {}",
                transaction.label,
                dollars(transaction.cost_cents)
            )
        } else {
            format!("Undid {}", transaction.label)
        };
        self.construction.redo.push(transaction);
        ActionResult::ok(message)
    }

    pub(crate) fn redo_construction(&mut self) -> ActionResult {
        let Some(transaction) = self.construction.redo.last().cloned() else {
            return ActionResult::error("Nothing to redo");
        };
        if let Err(reason) = self.check_redo(&transaction) {
            return ActionResult::error(format!("Cannot redo {}: {reason}", transaction.label));
        }
        if let Err(reason) = self.apply_redo(&transaction) {
            return ActionResult::error(format!("Cannot redo {}: {reason}", transaction.label));
        }
        self.construction.redo.pop();
        let message = if transaction.cost_cents > 0 {
            format!(
                "Redid {} · charged {}",
                transaction.label,
                dollars(transaction.cost_cents)
            )
        } else {
            format!("Redid {}", transaction.label)
        };
        self.construction.undo.push(transaction);
        ActionResult::ok(message)
    }
}

fn habitat_tiles(habitat: &Habitat) -> Vec<Position> {
    (habitat.y..habitat.y + habitat.height)
        .flat_map(|y| (habitat.x..habitat.x + habitat.width).map(move |x| Position { x, y }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(tiles: &[Position]) -> Vec<(u32, u32)> {
        tiles.iter().map(|tile| (tile.x, tile.y)).collect()
    }

    #[test]
    fn strokes_join_points_without_gaps_or_repeats() {
        let tiles = stroke_tiles(&[2, 2, 5, 3, 5, 3, 2, 2]).unwrap();
        assert_eq!(
            flat(&tiles),
            vec![(2, 2), (3, 2), (3, 3), (4, 3), (5, 3), (4, 2)]
        );
        for pair in tiles.windows(2).take(4) {
            let distance = pair[0].x.abs_diff(pair[1].x) + pair[0].y.abs_diff(pair[1].y);
            assert_eq!(distance, 1, "stroke is 4-connected");
        }
    }

    #[test]
    fn strokes_are_bounded() {
        assert!(stroke_tiles(&[]).is_err());
        assert!(stroke_tiles(&[1]).is_err());
        assert!(stroke_tiles(&[0, 0, u32::MAX, 0]).is_err());
        let zigzag: Vec<u32> = (0..300).flat_map(|i| [i % 20, i / 20]).collect();
        assert!(stroke_tiles(&zigzag).is_err());
    }
}
