//! Habitat environment (#97): terrain surfaces, bounded elevation, shelters and enrichment.
//!
//! Every habitat owns its environment. Terrain is stored sparsely: a tile without a cell is
//! level grass. Elevation is a whole number of steps, `0..=MAX_HEIGHT`. Objects are one-tile
//! shelters or enrichment items with park-wide stable IDs and an owned copy of their content
//! definition, like a habitat's fence.
//!
//! What the animals can use is decided here, deterministically, from the service gate's inside
//! tile: land and shallow water are walkable, deep water is never walked on, and a step of
//! more than [`MAX_WALK_STEP`] between neighbouring tiles is a ledge. The tiles reached from
//! the gate are the usable area; deep water counts when it touches reachable ground, and an
//! object counts only when its tile is reachable. Paint or decoration outside these rules
//! (the cosmetic scenery, for example) never satisfies a requirement.
//!
//! Every edit is planned on a copy and committed whole: it is rejected, with nothing spent,
//! when it would strand an animal or keeper, flood the gate, sink an object, raise a
//! fence-side tile or create a slope steeper than [`MAX_SLOPE_STEP`].
use super::{
    ActionResult, ExpenseCategory, GameState, Habitat, Position,
    content::{HabitatObjectType, ObjectRole, Surface},
    shape,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// The highest elevation step (each step is half a metre of authored height).
pub(crate) const MAX_HEIGHT: u32 = 4;
/// Animals climb at most this many steps between neighbouring tiles.
pub(crate) const MAX_WALK_STEP: u32 = 1;
/// No two neighbouring tiles may differ by more than this many steps.
pub(crate) const MAX_SLOPE_STEP: u32 = 2;
/// Objects one habitat can hold.
pub(crate) const MAX_OBJECTS: usize = 8;
/// Tiles one terrain edit may touch: a whole largest habitat.
const MAX_EDIT_TILES: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerrainCell {
    pub x: u32,
    pub y: u32,
    pub surface: Surface,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HabitatObject {
    pub id: u32,
    pub definition: HabitatObjectType,
    pub position: Position,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Environment {
    /// Tiles that are not level grass, row-major.
    pub terrain: Vec<TerrainCell>,
    pub objects: Vec<HabitatObject>,
}

impl Environment {
    pub(crate) fn is_default(&self) -> bool {
        self.terrain.is_empty() && self.objects.is_empty()
    }

    fn index(&self, tile: Position) -> Result<usize, usize> {
        self.terrain
            .binary_search_by_key(&(tile.y, tile.x), |cell| (cell.y, cell.x))
    }

    /// The tile's surface and height; a tile without a cell is level grass.
    pub(crate) fn cell(&self, tile: Position) -> (Surface, u32) {
        self.index(tile).map_or((Surface::Grass, 0), |at| {
            (self.terrain[at].surface, self.terrain[at].height)
        })
    }

    fn set(&mut self, tile: Position, surface: Surface, height: u32) {
        let default = surface == Surface::Grass && height == 0;
        match self.index(tile) {
            Ok(at) if default => {
                self.terrain.remove(at);
            }
            Ok(at) => {
                self.terrain[at].surface = surface;
                self.terrain[at].height = height;
            }
            Err(_) if default => {}
            Err(at) => self.terrain.insert(
                at,
                TerrainCell {
                    x: tile.x,
                    y: tile.y,
                    surface,
                    height,
                },
            ),
        }
    }

    pub(crate) fn object_at(&self, tile: Position) -> Option<&HabitatObject> {
        self.objects.iter().find(|object| object.position == tile)
    }

    /// Drops terrain outside `tiles` (after a reshape); objects must already fit.
    pub(crate) fn keep_within(&mut self, tiles: &[Position]) {
        self.terrain.retain(|cell| {
            shape::contains(
                tiles,
                Position {
                    x: cell.x,
                    y: cell.y,
                },
            )
        });
    }
}

fn neighbours(tile: Position) -> impl Iterator<Item = Position> {
    [(0i64, -1i64), (1, 0), (0, 1), (-1, 0)]
        .into_iter()
        .filter_map(move |(dx, dy)| {
            Some(Position {
                x: u32::try_from(i64::from(tile.x) + dx).ok()?,
                y: u32::try_from(i64::from(tile.y) + dy).ok()?,
            })
        })
}

/// Where the animals of one habitat can go: the walkable tiles reached from the gate.
#[derive(Clone, Debug)]
pub(crate) struct RoamArea {
    /// Reachable walkable tiles, row-major.
    pub tiles: Vec<Position>,
    set: HashSet<(u32, u32)>,
    heights: HashMap<(u32, u32), u32>,
    /// Reachable tiles clear of the fence; targets are chosen from these when there are any.
    pub targets: Vec<Position>,
}

impl RoamArea {
    pub(crate) fn contains(&self, tile: Position) -> bool {
        self.set.contains(&(tile.x, tile.y))
    }

    fn passable(&self, from: Position, to: Position) -> bool {
        let height = |tile: Position| self.heights.get(&(tile.x, tile.y)).copied();
        match (height(from), height(to)) {
            (Some(a), Some(b)) => a.abs_diff(b) <= MAX_WALK_STEP,
            _ => false,
        }
    }

    /// The next tile from `from` towards `to` through the area, or `from` once there or when
    /// no route exists.
    pub(crate) fn step_towards(&self, from: Position, to: Position) -> Position {
        if from == to || !self.contains(to) {
            return from;
        }
        // Breadth-first from the target, so the first neighbour of `from` reached is a
        // shortest step.
        let mut seen = HashSet::from([(to.x, to.y)]);
        let mut queue = VecDeque::from([to]);
        while let Some(tile) = queue.pop_front() {
            for next in neighbours(tile) {
                if !self.passable(tile, next) {
                    continue;
                }
                if next == from {
                    return tile;
                }
                if seen.insert((next.x, next.y)) {
                    queue.push_back(next);
                }
            }
        }
        from
    }

    /// The tiles animals choose targets from: those clear of the fence, or every reachable
    /// tile when none are.
    pub(crate) fn pool(&self) -> &[Position] {
        if self.targets.is_empty() {
            &self.tiles
        } else {
            &self.targets
        }
    }

    /// The reachable target tile nearest `from` (Manhattan distance, then row-major order).
    pub(crate) fn nearest(&self, from: Position) -> Position {
        self.pool()
            .iter()
            .copied()
            .min_by_key(|tile| {
                (
                    tile.x.abs_diff(from.x) + tile.y.abs_diff(from.y),
                    tile.y,
                    tile.x,
                )
            })
            .unwrap_or(from)
    }
}

/// The roam area of an enclosure of `tiles` whose gate opens at `root`.
pub(crate) fn roam_area(tiles: &[Position], environment: &Environment, root: Position) -> RoamArea {
    let inside = shape::tile_set(tiles);
    let walkable = |tile: Position| {
        inside.contains(&(tile.x, tile.y)) && environment.cell(tile).0 != Surface::DeepWater
    };
    let mut heights = HashMap::new();
    let mut set = HashSet::new();
    if walkable(root) {
        set.insert((root.x, root.y));
        heights.insert((root.x, root.y), environment.cell(root).1);
        let mut queue = VecDeque::from([root]);
        while let Some(tile) = queue.pop_front() {
            let height = environment.cell(tile).1;
            for next in neighbours(tile) {
                if !walkable(next) || set.contains(&(next.x, next.y)) {
                    continue;
                }
                let next_height = environment.cell(next).1;
                if height.abs_diff(next_height) > MAX_WALK_STEP {
                    continue;
                }
                set.insert((next.x, next.y));
                heights.insert((next.x, next.y), next_height);
                queue.push_back(next);
            }
        }
    }
    let reachable: Vec<Position> = tiles
        .iter()
        .copied()
        .filter(|tile| set.contains(&(tile.x, tile.y)))
        .collect();
    let targets = shape::inner_tiles(tiles)
        .into_iter()
        .filter(|tile| set.contains(&(tile.x, tile.y)))
        .collect();
    RoamArea {
        tiles: reachable,
        set,
        heights,
        targets,
    }
}

/// What a habitat's layout offers its animals, counted from the service gate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Assessment {
    /// Walkable tiles reached from the gate.
    pub reachable_tiles: u32,
    /// Walkable tiles cut off from the gate by water or ledges: no benefit.
    pub unreachable_tiles: u32,
    /// Reachable dry land not covered by an object: the space score's area.
    pub usable_land: u32,
    /// Usable land of the species' preferred ground cover.
    pub cover_tiles: u32,
    pub cover_percent: u32,
    /// Reachable shallow water tiles.
    pub shallow_water: u32,
    /// Deep water tiles next to reachable ground no more than one step up.
    pub deep_water: u32,
    pub shelter_capacity: u32,
    pub enrichment_capacity: u32,
    /// Objects on tiles the animals cannot reach: they provide nothing.
    pub unreachable_objects: Vec<u32>,
}

impl Habitat {
    /// The service gate's inside tile: where keepers enter and reachability starts.
    pub(crate) fn gate_tile(&self) -> Position {
        Position {
            x: self.service_gate.x,
            y: self.service_gate.y,
        }
    }

    pub(crate) fn roam_area(&self) -> RoamArea {
        roam_area(&self.tiles, &self.environment, self.gate_tile())
    }

    pub(crate) fn assessment(&self) -> Assessment {
        let area = self.roam_area();
        let environment = &self.environment;
        let cover = self
            .species
            .as_ref()
            .map_or(Surface::Grass, |species| species.habitat.ground_cover);
        let mut assessment = Assessment::default();
        for tile in &self.tiles {
            let (surface, _) = environment.cell(*tile);
            let reachable = area.contains(*tile);
            match surface {
                Surface::DeepWater => {
                    let usable = neighbours(*tile).any(|next| {
                        area.contains(next) && environment.cell(next).1 <= MAX_WALK_STEP
                    });
                    if usable {
                        assessment.deep_water += 1;
                    }
                }
                _ if !reachable => assessment.unreachable_tiles += 1,
                Surface::ShallowWater => {
                    assessment.reachable_tiles += 1;
                    assessment.shallow_water += 1;
                }
                Surface::Grass | Surface::Sand => {
                    assessment.reachable_tiles += 1;
                    if environment.object_at(*tile).is_none() {
                        assessment.usable_land += 1;
                        if surface == cover {
                            assessment.cover_tiles += 1;
                        }
                    }
                }
            }
        }
        assessment.cover_percent = (assessment.cover_tiles * 100)
            .checked_div(assessment.usable_land)
            .unwrap_or(0);
        for object in &environment.objects {
            if !area.contains(object.position) {
                assessment.unreachable_objects.push(object.id);
                continue;
            }
            let capacity = match object.definition.role {
                ObjectRole::Shelter => &mut assessment.shelter_capacity,
                ObjectRole::Enrichment => &mut assessment.enrichment_capacity,
            };
            *capacity = capacity.saturating_add(object.definition.capacity);
        }
        assessment
    }

    pub(crate) fn has_reachable_shelter(&self) -> bool {
        self.assessment().shelter_capacity > 0
    }
}

/// One weighted part of a habitat's welfare target, explained.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Contribution {
    pub key: &'static str,
    pub label: &'static str,
    pub weight: u32,
    pub score: u32,
    /// Whether the species' requirement is fully met (or there is none).
    pub met: bool,
    pub detail: String,
}

fn ratio(provided: u32, required: u32) -> u32 {
    (provided.min(required) * 100)
        .checked_div(required)
        .unwrap_or(100)
}

fn plural(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl Habitat {
    /// Every part of the welfare target with its weight, score and explanation.
    pub(crate) fn welfare_parts(&self) -> Vec<Contribution> {
        let assessment = self.assessment();
        let stocked = self.stocked();
        let animals = self.animal_count();
        let part = |key, label, weight, score: u32, detail: String| Contribution {
            key,
            label,
            weight,
            score,
            met: score >= 100,
            detail,
        };
        let mut parts = vec![
            part(
                "social",
                "Social group",
                2,
                self.social_score(),
                self.species.as_ref().map_or_else(
                    || "No animals yet".to_owned(),
                    |species| {
                        format!(
                            "{animals} of at least {} together",
                            species.welfare_profile().minimum_social_group
                        )
                    },
                ),
            ),
            part(
                "space",
                "Usable space",
                2,
                self.space_score_with(&assessment),
                self.species.as_ref().map_or_else(
                    || format!("{} usable dry tiles", assessment.usable_land),
                    |species| {
                        format!(
                            "{} usable dry tiles reachable from the gate; {} animals need {}",
                            assessment.usable_land,
                            animals,
                            animals.saturating_mul(species.welfare_profile().space_per_animal)
                        )
                    },
                ),
            ),
            part(
                "food",
                "Food",
                1,
                self.food_score(),
                format!("Feed level {}", self.food),
            ),
            part(
                "water",
                "Drinking water",
                1,
                self.water_score(),
                format!("Trough level {}", self.water),
            ),
            part(
                "cleanliness",
                "Cleanliness",
                1,
                self.cleanliness_score(),
                format!("Cleanliness {}", self.cleanliness),
            ),
        ];
        let Some(species) = self.species.as_ref().filter(|_| stocked) else {
            for (key, label) in [
                ("shelter", "Shelter"),
                ("enrichment", "Enrichment"),
                ("ground_cover", "Ground cover"),
                ("shallow_water", "Shallow water"),
                ("deep_water", "Deep water"),
            ] {
                parts.push(part(
                    key,
                    label,
                    1,
                    100,
                    "Needs depend on the animals".to_owned(),
                ));
            }
            return parts;
        };
        let needs = &species.habitat;
        let unreachable = |role: ObjectRole| {
            let count = self
                .environment
                .objects
                .iter()
                .filter(|object| {
                    object.definition.role == role
                        && assessment.unreachable_objects.contains(&object.id)
                })
                .count();
            if count == 0 {
                String::new()
            } else {
                format!(" · {count} unreachable, no benefit")
            }
        };
        let (score, detail) = if needs.shelter {
            (
                40 + 60 * ratio(assessment.shelter_capacity, animals) / 100,
                format!(
                    "Reachable shelter for {} of {animals} animals{}",
                    assessment.shelter_capacity.min(animals),
                    unreachable(ObjectRole::Shelter)
                ),
            )
        } else {
            (100, "Not needed by this species".to_owned())
        };
        parts.push(part("shelter", "Shelter", 1, score, detail));
        let (score, detail) = if needs.enrichment {
            (
                50 + 50 * ratio(assessment.enrichment_capacity, animals) / 100,
                format!(
                    "Reachable enrichment for {} of {animals} animals{}",
                    assessment.enrichment_capacity.min(animals),
                    unreachable(ObjectRole::Enrichment)
                ),
            )
        } else {
            (100, "Not needed by this species".to_owned())
        };
        parts.push(part("enrichment", "Enrichment", 1, score, detail));
        parts.push(part(
            "ground_cover",
            "Ground cover",
            1,
            ratio(assessment.cover_percent, needs.ground_cover_percent),
            format!(
                "{}% of usable land is {}; needs {}%",
                assessment.cover_percent,
                needs.ground_cover.label(),
                needs.ground_cover_percent
            ),
        ));
        for (key, label, provided, required, place) in [
            (
                "shallow_water",
                "Shallow water",
                assessment.shallow_water,
                needs.shallow_water_tiles,
                "reachable",
            ),
            (
                "deep_water",
                "Deep water",
                assessment.deep_water,
                needs.deep_water_tiles,
                "beside reachable ground",
            ),
        ] {
            let detail = if required == 0 {
                format!("Not needed · {}", plural(provided, "tile", "tiles"))
            } else {
                format!(
                    "{} {place}; needs {required}",
                    plural(provided, "tile", "tiles")
                )
            };
            parts.push(part(key, label, 1, ratio(provided, required), detail));
        }
        parts
    }

    /// The weighted welfare target of these parts.
    pub(crate) fn target_of(parts: &[Contribution]) -> u32 {
        let weight: u32 = parts.iter().map(|part| part.weight).sum();
        parts
            .iter()
            .map(|part| part.weight * part.score)
            .sum::<u32>()
            .checked_div(weight)
            .unwrap_or(100)
    }

    /// The first unmet habitat-layout requirement, explained.
    pub(crate) fn environment_problem(&self) -> Option<String> {
        if !self.stocked() {
            return None;
        }
        self.welfare_parts()
            .into_iter()
            .filter(|part| {
                matches!(
                    part.key,
                    "shelter" | "enrichment" | "ground_cover" | "shallow_water" | "deep_water"
                )
            })
            .find(|part| !part.met)
            .map(|part| format!("{}: {}", part.label, part.detail))
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ObjectView {
    pub id: u32,
    pub key: String,
    pub label: String,
    pub role: ObjectRole,
    pub capacity: u32,
    pub x: u32,
    pub y: u32,
    /// Whether the animals can reach it; an unreachable object provides nothing.
    pub reachable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EnvironmentView {
    /// Tiles that are not level grass, row-major.
    pub terrain: Vec<TerrainCell>,
    pub objects: Vec<ObjectView>,
    pub assessment: Assessment,
    /// Reachable walkable tiles, row-major: the welfare overlay of usable ground.
    pub reachable: Vec<Position>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SurfaceOffer {
    pub key: &'static str,
    pub label: &'static str,
    pub tile_cents: i64,
    pub water: bool,
}

/// What a player can design habitats with.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct EnvironmentCatalogView {
    pub surfaces: Vec<SurfaceOffer>,
    pub elevation_step_cents: i64,
    pub max_height: u32,
    pub max_walk_step: u32,
    pub max_slope_step: u32,
    pub max_objects: usize,
    pub objects: Vec<HabitatObjectType>,
}

impl EnvironmentCatalogView {
    pub(crate) fn new(catalog: &super::Catalog) -> Self {
        let costs = &catalog.balance.habitat_environment;
        Self {
            surfaces: Surface::ALL
                .into_iter()
                .map(|surface| SurfaceOffer {
                    key: surface.key(),
                    label: surface.label(),
                    tile_cents: costs.surface_cents(surface),
                    water: surface.is_water(),
                })
                .collect(),
            elevation_step_cents: costs.elevation_step_cents,
            max_height: MAX_HEIGHT,
            max_walk_step: MAX_WALK_STEP,
            max_slope_step: MAX_SLOPE_STEP,
            max_objects: MAX_OBJECTS,
            objects: catalog.habitat_objects.clone(),
        }
    }
}

impl Habitat {
    pub(crate) fn environment_view(&self) -> EnvironmentView {
        let area = self.roam_area();
        EnvironmentView {
            terrain: self.environment.terrain.clone(),
            objects: self
                .environment
                .objects
                .iter()
                .map(|object| ObjectView {
                    id: object.id,
                    key: object.definition.key.clone(),
                    label: object.definition.label.clone(),
                    role: object.definition.role,
                    capacity: object.definition.capacity,
                    x: object.position.x,
                    y: object.position.y,
                    reachable: area.contains(object.position),
                })
                .collect(),
            assessment: self.assessment(),
            reachable: area.tiles,
        }
    }
}

/// The preview of one habitat environment edit; the matching command plans again.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct EnvironmentPreview {
    pub ok: bool,
    pub message: String,
    pub habitat_id: u32,
    pub cost_cents: i64,
    /// Tiles the edit changes.
    pub changed_tiles: Vec<Position>,
    /// Tiles that make the edit invalid.
    pub blocking_tiles: Vec<Position>,
    /// The layout and welfare target after the edit.
    pub assessment: Option<Assessment>,
    pub welfare_target: Option<u32>,
}

impl EnvironmentPreview {
    fn rejected(habitat_id: u32, message: impl Into<String>, blocking: Vec<Position>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            habitat_id,
            cost_cents: 0,
            changed_tiles: Vec::new(),
            blocking_tiles: blocking,
            assessment: None,
            welfare_target: None,
        }
    }
}

/// The edits players can make to a habitat's environment.
#[derive(Clone, Debug)]
pub(crate) enum Edit {
    Paint(Surface),
    Elevate(u32),
}

fn edit_tiles(points: &[u32]) -> Result<Vec<Position>, String> {
    if points.is_empty() || !points.len().is_multiple_of(2) {
        return Err("Pick at least one tile".to_owned());
    }
    if points.len() > 2 * MAX_EDIT_TILES {
        return Err(format!("Edit at most {MAX_EDIT_TILES} tiles at once"));
    }
    let mut tiles: Vec<Position> = points
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| Position { x, y })
        .collect();
    tiles.sort_by_key(|tile| (tile.y, tile.x));
    tiles.dedup();
    if tiles.len() > MAX_EDIT_TILES {
        return Err(format!("Edit at most {MAX_EDIT_TILES} tiles at once"));
    }
    Ok(tiles)
}

/// The tile a default shelter goes to: the first free reachable dry tile, scanning rows from
/// the north and each row from the east, skipping the gate tile.
pub(crate) fn default_object_tile(
    tiles: &[Position],
    environment: &Environment,
    root: Position,
) -> Option<Position> {
    let area = roam_area(tiles, environment, root);
    let mut candidates = tiles.to_vec();
    candidates.sort_by_key(|tile| (tile.y, std::cmp::Reverse(tile.x)));
    candidates.into_iter().find(|tile| {
        *tile != root
            && area.contains(*tile)
            && !environment.cell(*tile).0.is_water()
            && environment.object_at(*tile).is_none()
    })
}

impl GameState {
    fn habitat_index(&self, habitat_id: u32) -> Option<usize> {
        self.habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
    }

    /// Why `after` (a habitat with an edited environment) is not a valid layout, with the
    /// tiles responsible.
    pub(crate) fn environment_violation(&self, after: &Habitat) -> Option<(String, Vec<Position>)> {
        let environment = &after.environment;
        let inner: HashSet<(u32, u32)> = shape::tile_set(&shape::inner_tiles(&after.tiles));
        for cell in &environment.terrain {
            let tile = Position {
                x: cell.x,
                y: cell.y,
            };
            if cell.height > MAX_HEIGHT {
                return Some((format!("Heights go from 0 to {MAX_HEIGHT}"), vec![tile]));
            }
            if cell.surface.is_water() && cell.height > 0 {
                return Some((
                    "Water lies on level ground (height 0)".to_owned(),
                    vec![tile],
                ));
            }
            if cell.height > 0 && !inner.contains(&(tile.x, tile.y)) {
                return Some((
                    "Tiles along the fence stay level so the fence stands on flat ground"
                        .to_owned(),
                    vec![tile],
                ));
            }
            if let Some(next) = neighbours(tile).find(|next| {
                after.contains(*next)
                    && environment.cell(*next).1.abs_diff(cell.height) > MAX_SLOPE_STEP
            }) {
                return Some((
                    format!(
                        "Too steep: neighbouring tiles differ by at most {MAX_SLOPE_STEP} steps"
                    ),
                    vec![tile, next],
                ));
            }
        }
        let gate = after.gate_tile();
        if environment.cell(gate).0.is_water() {
            return Some((
                "The service gate must open onto dry land".to_owned(),
                vec![gate],
            ));
        }
        let mut seen = HashSet::new();
        for object in &environment.objects {
            if !after.contains(object.position) {
                return Some((
                    format!(
                        "{} #{} would be outside the fence",
                        object.definition.label, object.id
                    ),
                    vec![object.position],
                ));
            }
            if environment.cell(object.position).0.is_water() {
                return Some((
                    format!(
                        "{} #{} cannot stand in water",
                        object.definition.label, object.id
                    ),
                    vec![object.position],
                ));
            }
            if !seen.insert((object.position.x, object.position.y)) {
                return Some(("One object per tile".to_owned(), vec![object.position]));
            }
        }
        let area = after.roam_area();
        if let Some(animal) = after
            .animals
            .iter()
            .find(|animal| !area.contains(animal.position))
        {
            return Some((
                format!(
                    "Would strand {} (#{}): every animal must stay on ground reachable from the gate",
                    animal.name, animal.id
                ),
                vec![animal.position],
            ));
        }
        if let Some(keeper) = self.keepers.iter().find(|keeper| {
            let tile = Position {
                x: keeper.x,
                y: keeper.y,
            };
            after.contains(tile) && !area.contains(tile)
        }) {
            return Some((
                format!("Would strand keeper #{} inside the habitat", keeper.id),
                vec![Position {
                    x: keeper.x,
                    y: keeper.y,
                }],
            ));
        }
        None
    }

    fn finish_preview(
        &self,
        before: &Habitat,
        after: &Habitat,
        cost_cents: i64,
        changed: Vec<Position>,
        verb: &str,
    ) -> EnvironmentPreview {
        let id = before.id;
        if let Some((message, blocking)) = self.environment_violation(after) {
            return EnvironmentPreview::rejected(id, message, blocking);
        }
        let parts = after.welfare_parts();
        let mut preview = EnvironmentPreview {
            ok: true,
            message: String::new(),
            habitat_id: id,
            cost_cents,
            changed_tiles: changed,
            blocking_tiles: Vec::new(),
            assessment: Some(after.assessment()),
            welfare_target: Some(Habitat::target_of(&parts)),
        };
        if preview.changed_tiles.is_empty() && before.environment == after.environment {
            preview.message = format!("Habitat #{id} already looks like this");
            return preview;
        }
        if self.cash_cents < cost_cents {
            preview.ok = false;
            preview.message = "Not enough cash".to_owned();
            return preview;
        }
        preview.message = format!("{verb} in habitat #{id}");
        preview
    }

    /// Plans a terrain edit of `points` (flat `x, y` pairs) in habitat `habitat_id`, returning
    /// the edited habitat when valid.
    fn plan_terrain(
        &self,
        habitat_id: u32,
        points: &[u32],
        edit: &Edit,
    ) -> (EnvironmentPreview, Option<Habitat>) {
        let Some(index) = self.habitat_index(habitat_id) else {
            return (
                EnvironmentPreview::rejected(habitat_id, "Select a habitat first", Vec::new()),
                None,
            );
        };
        let before = &self.habitats[index];
        let tiles = match edit_tiles(points) {
            Ok(tiles) => tiles,
            Err(message) => {
                return (
                    EnvironmentPreview::rejected(habitat_id, message, Vec::new()),
                    None,
                );
            }
        };
        let outside: Vec<Position> = tiles
            .iter()
            .copied()
            .filter(|t| !before.contains(*t))
            .collect();
        if !outside.is_empty() {
            return (
                EnvironmentPreview::rejected(
                    habitat_id,
                    format!("Edit only tiles inside habitat #{habitat_id}"),
                    outside,
                ),
                None,
            );
        }
        let costs = &self.catalog.balance.habitat_environment;
        let mut after = before.clone();
        let mut cost: i64 = 0;
        let mut changed = Vec::new();
        for tile in tiles {
            let (surface, height) = before.environment.cell(tile);
            let (new_surface, new_height) = match edit {
                Edit::Paint(new) => (*new, height),
                Edit::Elevate(new) => (surface, *new),
            };
            if (new_surface, new_height) == (surface, height) {
                continue;
            }
            if let Edit::Elevate(new) = edit {
                if *new > MAX_HEIGHT {
                    return (
                        EnvironmentPreview::rejected(
                            habitat_id,
                            format!("Heights go from 0 to {MAX_HEIGHT}"),
                            vec![tile],
                        ),
                        None,
                    );
                }
                if surface.is_water() {
                    return (
                        EnvironmentPreview::rejected(
                            habitat_id,
                            "Water lies on level ground; paint it as land before raising it",
                            vec![tile],
                        ),
                        None,
                    );
                }
                cost = cost.saturating_add(
                    costs
                        .elevation_step_cents
                        .saturating_mul(i64::from(height.abs_diff(*new))),
                );
            } else {
                if new_surface.is_water() && height > 0 {
                    return (
                        EnvironmentPreview::rejected(
                            habitat_id,
                            "Water lies on level ground; lower the tile to height 0 first",
                            vec![tile],
                        ),
                        None,
                    );
                }
                if new_surface.is_water()
                    && let Some(object) = before.environment.object_at(tile)
                {
                    return (
                        EnvironmentPreview::rejected(
                            habitat_id,
                            format!(
                                "{} #{} stands here; remove it before flooding the tile",
                                object.definition.label, object.id
                            ),
                            vec![tile],
                        ),
                        None,
                    );
                }
                cost = cost.saturating_add(costs.surface_cents(new_surface));
            }
            after.environment.set(tile, new_surface, new_height);
            changed.push(tile);
        }
        let verb = match edit {
            Edit::Paint(surface) => format!(
                "Paint {} {}",
                plural(changed.len() as u32, "tile", "tiles"),
                surface.label()
            ),
            Edit::Elevate(height) => format!(
                "Set {} to height {height}",
                plural(changed.len() as u32, "tile", "tiles")
            ),
        };
        let preview = self.finish_preview(before, &after, cost, changed, &verb);
        let valid = preview.ok;
        (preview, valid.then_some(after))
    }

    pub(crate) fn evaluate_habitat_terrain(
        &self,
        habitat_id: u32,
        points: &[u32],
        surface: &str,
    ) -> EnvironmentPreview {
        match Surface::parse(surface) {
            Some(surface) => {
                self.plan_terrain(habitat_id, points, &Edit::Paint(surface))
                    .0
            }
            None => EnvironmentPreview::rejected(
                habitat_id,
                format!("Unknown surface {surface}"),
                Vec::new(),
            ),
        }
    }

    pub(crate) fn evaluate_habitat_elevation(
        &self,
        habitat_id: u32,
        points: &[u32],
        height: u32,
    ) -> EnvironmentPreview {
        self.plan_terrain(habitat_id, points, &Edit::Elevate(height))
            .0
    }

    pub(crate) fn paint_habitat_terrain(
        &mut self,
        habitat_id: u32,
        points: &[u32],
        surface: &str,
    ) -> ActionResult {
        let Some(surface) = Surface::parse(surface) else {
            return ActionResult::error(format!("Unknown surface {surface}"));
        };
        self.commit_terrain(habitat_id, points, &Edit::Paint(surface))
    }

    pub(crate) fn set_habitat_elevation(
        &mut self,
        habitat_id: u32,
        points: &[u32],
        height: u32,
    ) -> ActionResult {
        self.commit_terrain(habitat_id, points, &Edit::Elevate(height))
    }

    fn commit_terrain(&mut self, habitat_id: u32, points: &[u32], edit: &Edit) -> ActionResult {
        let (preview, after) = self.plan_terrain(habitat_id, points, edit);
        let Some(after) = after.filter(|_| preview.ok) else {
            return ActionResult::error(preview.message);
        };
        if preview.changed_tiles.is_empty() {
            return ActionResult::ok(preview.message);
        }
        if let Err(message) = self.spend(preview.cost_cents, ExpenseCategory::Construction) {
            return ActionResult::error(message);
        }
        self.replace_environment(habitat_id, after.environment);
        ActionResult::ok(preview.message)
    }

    /// Installs a validated environment and keeps walking animals heading somewhere reachable.
    fn replace_environment(&mut self, habitat_id: u32, environment: Environment) {
        let Some(index) = self.habitat_index(habitat_id) else {
            return;
        };
        let habitat = &mut self.habitats[index];
        habitat.environment = environment;
        let area = habitat.roam_area();
        for animal in &mut habitat.animals {
            animal.settle_into(&area);
        }
        self.emit("habitat_environment_changed", Some(habitat_id));
    }

    fn plan_object(
        &self,
        habitat_id: u32,
        key: &str,
        at: Option<Position>,
    ) -> (EnvironmentPreview, Option<Habitat>) {
        let Some(index) = self.habitat_index(habitat_id) else {
            return (
                EnvironmentPreview::rejected(habitat_id, "Select a habitat first", Vec::new()),
                None,
            );
        };
        let before = &self.habitats[index];
        let Some(definition) = self.catalog.habitat_objects.iter().find(|o| o.key == key) else {
            return (
                EnvironmentPreview::rejected(
                    habitat_id,
                    format!("Unknown habitat object {key}"),
                    Vec::new(),
                ),
                None,
            );
        };
        if before.environment.objects.len() >= MAX_OBJECTS {
            return (
                EnvironmentPreview::rejected(
                    habitat_id,
                    format!("A habitat holds at most {MAX_OBJECTS} shelters and enrichment items"),
                    Vec::new(),
                ),
                None,
            );
        }
        let Some(tile) = at.or_else(|| {
            default_object_tile(&before.tiles, &before.environment, before.gate_tile())
        }) else {
            return (
                EnvironmentPreview::rejected(
                    habitat_id,
                    "No free dry tile is reachable",
                    Vec::new(),
                ),
                None,
            );
        };
        if !before.contains(tile) {
            return (
                EnvironmentPreview::rejected(
                    habitat_id,
                    format!("Place it inside habitat #{habitat_id}"),
                    vec![tile],
                ),
                None,
            );
        }
        if tile == before.gate_tile() {
            return (
                EnvironmentPreview::rejected(
                    habitat_id,
                    "Keep the service gate tile clear",
                    vec![tile],
                ),
                None,
            );
        }
        if let Some(object) = before.environment.object_at(tile) {
            return (
                EnvironmentPreview::rejected(
                    habitat_id,
                    format!(
                        "{} #{} already stands here",
                        object.definition.label, object.id
                    ),
                    vec![tile],
                ),
                None,
            );
        }
        let mut after = before.clone();
        after.environment.objects.push(HabitatObject {
            id: self.next_habitat_object_id,
            definition: definition.clone(),
            position: tile,
        });
        let mut preview = self.finish_preview(
            before,
            &after,
            definition.build_cents,
            vec![tile],
            &format!("Place {} at {},{}", definition.label, tile.x, tile.y),
        );
        if preview.ok && !after.roam_area().contains(tile) {
            preview
                .message
                .push_str(" · unreachable from the gate: it will not help the animals");
        }
        let valid = preview.ok;
        (preview, valid.then_some(after))
    }

    pub(crate) fn evaluate_habitat_object(
        &self,
        habitat_id: u32,
        key: &str,
        x: u32,
        y: u32,
    ) -> EnvironmentPreview {
        self.plan_object(habitat_id, key, Some(Position { x, y })).0
    }

    pub(crate) fn place_habitat_object(
        &mut self,
        habitat_id: u32,
        key: &str,
        x: u32,
        y: u32,
    ) -> ActionResult {
        self.commit_object(habitat_id, key, Some(Position { x, y }))
    }

    fn commit_object(&mut self, habitat_id: u32, key: &str, at: Option<Position>) -> ActionResult {
        let (preview, after) = self.plan_object(habitat_id, key, at);
        let Some(after) = after.filter(|_| preview.ok) else {
            return ActionResult::error(preview.message);
        };
        if let Err(message) = self.spend(preview.cost_cents, ExpenseCategory::HabitatCare) {
            return ActionResult::error(message);
        }
        self.next_habitat_object_id = self.next_habitat_object_id.saturating_add(1);
        self.replace_environment(habitat_id, after.environment);
        ActionResult::ok(preview.message)
    }

    /// The one-click shelter: the catalog's first shelter on the default tile, unless the
    /// habitat already has a reachable shelter.
    pub(crate) fn add_shelter(&mut self, habitat_id: u32) -> ActionResult {
        let Some(index) = self.habitat_index(habitat_id) else {
            return ActionResult::error("Select a habitat first");
        };
        if self.habitats[index].has_reachable_shelter() {
            return ActionResult::ok("A reachable shelter is already installed");
        }
        let Some(key) = self
            .catalog
            .habitat_objects
            .iter()
            .find(|object| object.role == ObjectRole::Shelter)
            .map(|object| object.key.clone())
        else {
            return ActionResult::error("The content has no shelter");
        };
        self.commit_object(habitat_id, &key, None)
    }

    /// Removes an object. Like demolition it is free and refunds nothing.
    pub(crate) fn remove_habitat_object(
        &mut self,
        habitat_id: u32,
        object_id: u32,
    ) -> ActionResult {
        let Some(index) = self.habitat_index(habitat_id) else {
            return ActionResult::error("Select a habitat first");
        };
        let mut environment = self.habitats[index].environment.clone();
        let Some(at) = environment.objects.iter().position(|o| o.id == object_id) else {
            return ActionResult::error(format!(
                "Habitat #{habitat_id} has no object #{object_id}"
            ));
        };
        let object = environment.objects.remove(at);
        self.replace_environment(habitat_id, environment);
        ActionResult::ok(format!("{} #{object_id} removed", object.definition.label))
    }

    /// Why the saved environments are inconsistent, if they are.
    pub(crate) fn environment_save_violation(&self) -> Option<String> {
        let mut ids = HashSet::new();
        for habitat in &self.habitats {
            let id = habitat.id;
            let environment = &habitat.environment;
            let ordered = environment
                .terrain
                .windows(2)
                .all(|pair| (pair[0].y, pair[0].x) < (pair[1].y, pair[1].x));
            if !ordered {
                return Some(format!("habitat {id} terrain is not row-major and unique"));
            }
            for cell in &environment.terrain {
                if !habitat.contains(Position {
                    x: cell.x,
                    y: cell.y,
                }) {
                    return Some(format!("habitat {id} terrain leaves its fence"));
                }
                if cell.surface == Surface::Grass && cell.height == 0 {
                    return Some(format!("habitat {id} stores a level grass cell"));
                }
            }
            if environment.objects.len() > MAX_OBJECTS {
                return Some(format!(
                    "habitat {id} holds more than {MAX_OBJECTS} objects"
                ));
            }
            for object in &environment.objects {
                if object.id == 0
                    || object.id >= self.next_habitat_object_id
                    || !ids.insert(object.id)
                {
                    return Some(format!(
                        "habitat object id {} is duplicated or not yet allocated",
                        object.id
                    ));
                }
                if !self.catalog.habitat_objects.contains(&object.definition) {
                    return Some(format!(
                        "habitat object {} ({}) does not match the content",
                        object.id, object.definition.key
                    ));
                }
            }
            if let Some((message, _)) = self.environment_violation(habitat) {
                return Some(format!("habitat {id}: {message}"));
            }
        }
        None
    }
}
