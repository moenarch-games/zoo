//! Validated, immutable content owned by a park. Presentation references are IDs
//! from Zoo's existing asset manifest; they never select simulation rules.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 128;
const MAX_STRING_BYTES: usize = 256;
pub(crate) const MAX_CENTS: i64 = 9_007_199_254_740_991;
// A park has at most MAX_MAP_TILES tiles and habitat capacity is twelve. Bound even
// a conservative whole-park appeal sum before any arithmetic is performed.
const MAX_RULE_VALUE: u32 = u32::MAX / (super::scenario::MAX_MAP_TILES * 12);
/// Upper bound on a guest-comfort facility's slots or bin size (#101).
pub(crate) const MAX_FACILITY_CAPACITY: u32 = 32;
/// Facility looks the park view composes from primitives itself, as it does for stands, rather
/// than from the deprecated placeholder model catalog (Atlas 5.1.3) (#101).
const SCENE_COMPOSED_PRESENTATIONS: &[&str] = &["toilet_block", "bench", "litter_bin"];
pub(crate) const DEFAULT_DOCUMENT: &str = include_str!("../content/catalog.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalog {
    pub ruleset_id: String,
    pub balance: Balance,
    pub fences: Vec<FenceType>,
    /// Shelters and enrichment a habitat can be furnished with (#97).
    pub habitat_objects: Vec<HabitatObjectType>,
    pub species: Vec<Species>,
    /// Which pairs of species may share one habitat, and why or why not (#108). A pair without
    /// an entry is not supported: a habitat keeps one species unless a profile says otherwise.
    #[serde(default)]
    pub species_compatibility: Vec<SpeciesPairing>,
    pub facilities: Vec<Facility>,
    pub unlocks: Vec<Unlock>,
}

/// An authored mixed-exhibit verdict for two species (#108): a simplified game profile, not
/// veterinary guidance. Residents of a compatible pair share the habitat's usable land,
/// shelter, enrichment, feed, water and cleanliness; each species keeps its own social group,
/// fence and layout requirements.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpeciesPairing {
    /// Two distinct species keys.
    pub species: [String; 2],
    pub compatible: bool,
    /// The player-facing explanation of the verdict.
    pub reason: String,
}

/// Whether two species may share a habitat (#108), explained.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PairingVerdict {
    /// The two species keys, sorted.
    pub species: [String; 2],
    pub compatible: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Species {
    pub key: String,
    pub label: String,
    pub purchase_cost_cents: i64,
    pub appeal: u32,
    pub minimum_social_group: u32,
    pub space_per_animal: u32,
    /// The lowest fence, in centimetres, a habitat needs before it can adopt this species.
    pub minimum_fence_height_cm: u32,
    /// Operating cost charged every in-game hour for each animal of this species.
    pub hourly_upkeep_cents: i64,
    pub asset_ref: String,
    pub unlock_ref: String,
    pub care: CareProfile,
    /// What a habitat must offer these animals (#97): authored game simplifications.
    pub habitat: HabitatRequirements,
}

/// The ground a habitat tile is made of (#97). Land is walkable; shallow water is walkable
/// wading water; deep water is never walked on and is used from an adjacent tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Surface {
    #[default]
    Grass,
    Sand,
    ShallowWater,
    DeepWater,
}

impl Surface {
    pub const ALL: [Self; 4] = [Self::Grass, Self::Sand, Self::ShallowWater, Self::DeepWater];

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|surface| surface.key() == value)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Sand => "sand",
            Self::ShallowWater => "shallow_water",
            Self::DeepWater => "deep_water",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Sand => "sand",
            Self::ShallowWater => "shallow water",
            Self::DeepWater => "deep water",
        }
    }

    pub fn is_water(self) -> bool {
        matches!(self, Self::ShallowWater | Self::DeepWater)
    }
}

/// A species' habitat needs. Every amount must be reachable from the service gate to count.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HabitatRequirements {
    /// The land surface the animals want underfoot.
    pub ground_cover: Surface,
    /// The share of usable land, in percent, that should be `ground_cover`.
    pub ground_cover_percent: u32,
    pub shallow_water_tiles: u32,
    pub deep_water_tiles: u32,
    /// Whether every animal needs a place in a reachable shelter.
    pub shelter: bool,
    /// Whether every animal needs a share of reachable enrichment.
    pub enrichment: bool,
}

/// What a habitat object does for the animals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObjectRole {
    Shelter,
    Enrichment,
}

/// A one-tile shelter or enrichment item a habitat can be furnished with (#97).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HabitatObjectType {
    pub key: String,
    pub label: String,
    pub role: ObjectRole,
    pub build_cents: i64,
    /// How many animals one object serves.
    pub capacity: u32,
}

/// Habitat water requirements are bounded by the largest habitat.
pub(crate) const MAX_REQUIRED_WATER_TILES: u32 = 50;
pub(crate) const MAX_OBJECT_CAPACITY: u32 = 12;

/// A fence a habitat can be enclosed with (#95). The first entry is the default every new
/// habitat is built with; habitats switch type through a fence upgrade.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FenceType {
    pub key: String,
    pub label: String,
    pub material: FenceMaterial,
    pub height_cm: u32,
    /// Price of each fence segment built with this type.
    pub segment_cents: i64,
    /// Operating cost of each fence segment every in-game hour.
    pub segment_hourly_cents: i64,
}

/// The fence looks the renderer supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FenceMaterial {
    Wood,
    Steel,
}

/// Fences are bounded well above any real enclosure height.
pub(crate) const MAX_FENCE_HEIGHT_CM: u32 = 1_000;

/// Recurring keeper work categories a habitat schedule can generate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CareCategory {
    Feeding,
    Cleaning,
    Inspection,
    Enrichment,
}

impl CareCategory {
    pub const ALL: [Self; 4] = [
        Self::Feeding,
        Self::Cleaning,
        Self::Inspection,
        Self::Enrichment,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Feeding => "feeding",
            Self::Cleaning => "cleaning",
            Self::Inspection => "inspection",
            Self::Enrichment => "enrichment",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "feeding" => Some(Self::Feeding),
            "cleaning" => Some(Self::Cleaning),
            "inspection" => Some(Self::Inspection),
            "enrichment" => Some(Self::Enrichment),
            _ => None,
        }
    }
}

/// Species care defaults copied into a habitat when it is first stocked. The habitat owns
/// its copy afterwards, so neither catalog edits nor other habitats can rewrite it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CareProfile {
    /// Keepers every in-habitat job of this species needs at once (two for dangerous animals).
    pub required_keepers: u32,
    pub schedule: Vec<CareTaskDefault>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CareTaskDefault {
    pub category: CareCategory,
    pub first_minute_of_day: u32,
    pub interval_minutes: u32,
    pub priority: u32,
    /// Minutes a keeper spends inside the habitat on one occurrence, after walking in.
    pub work_minutes: u32,
}

pub(crate) const MAX_CARE_PRIORITY: u32 = 5;
pub(crate) const MAX_CARE_WORK_MINUTES: u32 = 60;
pub(crate) const MAX_REQUIRED_KEEPERS: u32 = 2;
/// The grace window is one in-game hour, so a cadence of at least an hour never lets two
/// occurrences of the same task be eligible at once. Dividing the day keeps times stable.
pub(crate) fn valid_care_timing(first_minute_of_day: u32, interval_minutes: u32) -> bool {
    first_minute_of_day < 24 * 60
        && (60..=24 * 60).contains(&interval_minutes)
        && (24 * 60) % interval_minutes == 0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Facility {
    pub key: String,
    pub label: String,
    pub service: String,
    pub asset_ref: String,
    pub unlock_ref: String,
    pub build_cost_cents: i64,
    pub price_cents: i64,
    /// Operating cost charged every in-game hour for each built stand. Fixed facilities are
    /// not built stands and keep this at zero.
    pub hourly_upkeep_cents: i64,
    /// What a guest-comfort facility holds at once (#101): toilet cubicles or bench seats, or
    /// the litter a bin takes before it must be emptied. Stands and landmarks declare 0.
    pub capacity: u32,
    pub footprint: Footprint,
    /// How a food or drink stand serves guests (#102); required for stands, absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stand: Option<StandService>,
}

/// A food or drink stand's service model (#102): bounded service time, a finite queue, a few
/// vendor windows and a finite stock that one delivery refills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StandService {
    /// Minutes one vendor needs to serve one guest at full condition.
    pub service_minutes: u32,
    /// Guests who may wait at the counter at once.
    pub queue_capacity: u32,
    /// Vendor windows: at most this many guests are served at once.
    pub max_vendors: u32,
    /// Portions the stand holds when fully stocked.
    pub stock_capacity: u32,
    /// What one delivered portion costs.
    pub stock_unit_cost_cents: i64,
}

/// Upper bounds of a stand's service model (#102).
pub(crate) const MAX_SERVICE_MINUTES: u32 = 10;
pub(crate) const MAX_QUEUE_CAPACITY: u32 = 12;
pub(crate) const MAX_VENDORS: u32 = 4;
pub(crate) const MAX_STOCK_CAPACITY: u32 = 200;

/// Park-wide prices, operating costs and wages. Species and facility costs live on their
/// own definitions; everything here applies to the whole park.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Balance {
    pub admission_price_cents: i64,
    pub construction: ConstructionCosts,
    pub habitat_care: HabitatCareCosts,
    pub habitat_environment: EnvironmentCosts,
    pub feed_batch_cents: i64,
    pub maintenance_repair_cents: i64,
    pub staff: StaffCosts,
    pub upkeep: ParkUpkeep,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConstructionCosts {
    pub path_tile_cents: i64,
    pub habitat_base_cents: i64,
    pub habitat_tile_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HabitatCareCosts {
    pub water_refill_cents: i64,
    pub cleaning_cents: i64,
}

/// Prices of habitat terrain edits (#97): per tile painted with a surface, and per height
/// step raised or lowered.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvironmentCosts {
    pub grass_tile_cents: i64,
    pub sand_tile_cents: i64,
    pub shallow_water_tile_cents: i64,
    pub deep_water_tile_cents: i64,
    pub elevation_step_cents: i64,
}

impl EnvironmentCosts {
    pub fn surface_cents(&self, surface: Surface) -> i64 {
        match surface {
            Surface::Grass => self.grass_tile_cents,
            Surface::Sand => self.sand_tile_cents,
            Surface::ShallowWater => self.shallow_water_tile_cents,
            Surface::DeepWater => self.deep_water_tile_cents,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StaffCosts {
    pub keeper: StaffRole,
    pub janitor: StaffRole,
    pub mechanic: StaffRole,
    /// Stand vendors (#102) are posted to a stand rather than hired at the depot, so they only
    /// cost wages.
    pub vendor: VendorRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VendorRole {
    pub hourly_wage_cents: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StaffRole {
    pub hire_cents: i64,
    pub hourly_wage_cents: i64,
}

/// Hourly park upkeep that no species or facility definition owns.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParkUpkeep {
    pub habitat_hourly_cents: i64,
}

impl Balance {
    fn validate(&self) -> Result<(), String> {
        let build = &self.construction;
        let care = &self.habitat_care;
        let environment = &self.habitat_environment;
        let staff = &self.staff;
        let upkeep = &self.upkeep;
        for (path, value) in [
            ("admission_price_cents", self.admission_price_cents),
            ("construction/path_tile_cents", build.path_tile_cents),
            ("construction/habitat_base_cents", build.habitat_base_cents),
            ("construction/habitat_tile_cents", build.habitat_tile_cents),
            ("habitat_care/water_refill_cents", care.water_refill_cents),
            ("habitat_care/cleaning_cents", care.cleaning_cents),
            (
                "habitat_environment/grass_tile_cents",
                environment.grass_tile_cents,
            ),
            (
                "habitat_environment/sand_tile_cents",
                environment.sand_tile_cents,
            ),
            (
                "habitat_environment/shallow_water_tile_cents",
                environment.shallow_water_tile_cents,
            ),
            (
                "habitat_environment/deep_water_tile_cents",
                environment.deep_water_tile_cents,
            ),
            (
                "habitat_environment/elevation_step_cents",
                environment.elevation_step_cents,
            ),
            ("feed_batch_cents", self.feed_batch_cents),
            ("maintenance_repair_cents", self.maintenance_repair_cents),
            ("staff/keeper/hire_cents", staff.keeper.hire_cents),
            (
                "staff/keeper/hourly_wage_cents",
                staff.keeper.hourly_wage_cents,
            ),
            ("staff/janitor/hire_cents", staff.janitor.hire_cents),
            (
                "staff/janitor/hourly_wage_cents",
                staff.janitor.hourly_wage_cents,
            ),
            ("staff/mechanic/hire_cents", staff.mechanic.hire_cents),
            (
                "staff/mechanic/hourly_wage_cents",
                staff.mechanic.hourly_wage_cents,
            ),
            (
                "staff/vendor/hourly_wage_cents",
                staff.vendor.hourly_wage_cents,
            ),
            ("upkeep/habitat_hourly_cents", upkeep.habitat_hourly_cents),
        ] {
            money(value, &format!("balance/{path}"))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Footprint {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Unlock {
    pub key: String,
    pub available: bool,
}

fn text(value: &str, path: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_STRING_BYTES {
        return Err(format!(
            "{path}: expected 1..={MAX_STRING_BYTES} UTF-8 bytes"
        ));
    }
    Ok(())
}

fn money(value: i64, path: &str) -> Result<(), String> {
    if !(0..=MAX_CENTS).contains(&value) {
        return Err(format!("{path}: expected 0..={MAX_CENTS} cents"));
    }
    Ok(())
}

impl Catalog {
    /// The authored verdict for two distinct species; a pair without a profile is unsupported.
    pub(crate) fn pairing(&self, a: &str, b: &str) -> PairingVerdict {
        let label = |key: &str| {
            self.species
                .iter()
                .find(|species| species.key == key)
                .map_or_else(|| key.to_owned(), |species| species.label.clone())
        };
        let mut species = [a.to_owned(), b.to_owned()];
        species.sort();
        let authored = self.species_compatibility.iter().find(|pairing| {
            let mut keys = pairing.species.clone();
            keys.sort();
            keys == species
        });
        match authored {
            Some(pairing) => PairingVerdict {
                species,
                compatible: pairing.compatible,
                reason: pairing.reason.clone(),
            },
            None => PairingVerdict {
                reason: format!(
                    "No mixed-exhibit profile pairs {} with {}; keep them in separate habitats",
                    label(a),
                    label(b)
                ),
                species,
                compatible: false,
            },
        }
    }

    pub fn parse(document: &str) -> Result<Self, String> {
        if document.len() > MAX_DOCUMENT_BYTES {
            return Err(format!("catalog: exceeds {MAX_DOCUMENT_BYTES} bytes"));
        }
        // Keep Serde's typed definition as the only schema authority. The
        // adapter records its field path without first building a parallel JSON
        // value tree or duplicating field/type tables.
        let mut deserializer = serde_json::Deserializer::from_str(document);
        let catalog: Self = serde_path_to_error::deserialize(&mut deserializer)
            .map_err(|error| format!("catalog/{}: {}", error.path(), error.inner()))?;
        deserializer
            .end()
            .map_err(|error| format!("catalog: {error}"))?;
        catalog.validate()?;
        Ok(catalog)
    }

    fn validate(&self) -> Result<(), String> {
        text(&self.ruleset_id, "ruleset_id")?;
        self.balance.validate()?;
        for (group, count) in [
            ("species", self.species.len()),
            ("facilities", self.facilities.len()),
            ("unlocks", self.unlocks.len()),
            ("fences", self.fences.len()),
            ("habitat_objects", self.habitat_objects.len()),
        ] {
            if count == 0 || count > MAX_ENTRIES {
                return Err(format!("{group}: expected 1..={MAX_ENTRIES} entries"));
            }
        }
        let manifest: Value = serde_json::from_str(include_str!("../../../assets/3d/catalog.json"))
            .expect("repository asset manifest is valid JSON");
        let assets: HashSet<&str> = manifest["assets"]
            .as_array()
            .expect("asset manifest entries")
            .iter()
            .filter_map(|entry| entry["id"].as_str())
            .chain(SCENE_COMPOSED_PRESENTATIONS.iter().copied())
            .collect();
        let mut unlocks = HashSet::new();
        for (i, unlock) in self.unlocks.iter().enumerate() {
            text(&unlock.key, &format!("unlocks/{i}/key"))?;
            if !unlocks.insert(unlock.key.as_str()) || !unlock.available {
                return Err(format!(
                    "unlocks/{i}: only unique existing available unlocks are supported"
                ));
            }
        }
        let mut fence_keys = HashSet::new();
        for (i, fence) in self.fences.iter().enumerate() {
            let prefix = format!("fences/{i}");
            text(&fence.key, &format!("{prefix}/key"))?;
            text(&fence.label, &format!("{prefix}/label"))?;
            if !fence
                .key
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(format!(
                    "{prefix}/key: expected stable lowercase identifier"
                ));
            }
            if !fence_keys.insert(fence.key.as_str()) {
                return Err(format!("{prefix}/key: duplicate fence key"));
            }
            if !(1..=MAX_FENCE_HEIGHT_CM).contains(&fence.height_cm) {
                return Err(format!(
                    "{prefix}/height_cm: expected 1..={MAX_FENCE_HEIGHT_CM}"
                ));
            }
            money(fence.segment_cents, &format!("{prefix}/segment_cents"))?;
            money(
                fence.segment_hourly_cents,
                &format!("{prefix}/segment_hourly_cents"),
            )?;
        }
        let mut object_keys = HashSet::new();
        for (i, object) in self.habitat_objects.iter().enumerate() {
            let prefix = format!("habitat_objects/{i}");
            text(&object.key, &format!("{prefix}/key"))?;
            text(&object.label, &format!("{prefix}/label"))?;
            if !object
                .key
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(format!(
                    "{prefix}/key: expected stable lowercase identifier"
                ));
            }
            if !object_keys.insert(object.key.as_str()) {
                return Err(format!("{prefix}/key: duplicate habitat object key"));
            }
            money(object.build_cents, &format!("{prefix}/build_cents"))?;
            if !(1..=MAX_OBJECT_CAPACITY).contains(&object.capacity) {
                return Err(format!(
                    "{prefix}/capacity: expected 1..={MAX_OBJECT_CAPACITY}"
                ));
            }
        }
        for (role, name) in [
            (ObjectRole::Shelter, "shelter"),
            (ObjectRole::Enrichment, "enrichment"),
        ] {
            if !self
                .habitat_objects
                .iter()
                .any(|object| object.role == role)
            {
                return Err(format!(
                    "habitat_objects: expected at least one {name} object"
                ));
            }
        }
        let tallest = self.fences.iter().map(|f| f.height_cm).max().unwrap_or(0);
        let mut keys = HashSet::new();
        for (i, species) in self.species.iter().enumerate() {
            let prefix = format!("species/{i}");
            money(
                species.purchase_cost_cents,
                &format!("{prefix}/purchase_cost_cents"),
            )?;
            money(
                species.hourly_upkeep_cents,
                &format!("{prefix}/hourly_upkeep_cents"),
            )?;
            self.references(
                &prefix,
                &species.key,
                &species.label,
                &species.asset_ref,
                &species.unlock_ref,
                &assets,
                &unlocks,
            )?;
            if !keys.insert(&species.key) {
                return Err(format!("{prefix}/key: duplicate species key"));
            }
            for (field, value) in [
                ("appeal", species.appeal),
                ("minimum_social_group", species.minimum_social_group),
                ("space_per_animal", species.space_per_animal),
            ] {
                if value == 0 || value > MAX_RULE_VALUE {
                    return Err(format!("{prefix}/{field}: expected 1..={MAX_RULE_VALUE}"));
                }
            }
            // Every species must be keepable behind at least one of the catalog's fences.
            if species.minimum_fence_height_cm > tallest {
                return Err(format!(
                    "{prefix}/minimum_fence_height_cm: no fence type is that tall"
                ));
            }
            let habitat = &species.habitat;
            if habitat.ground_cover.is_water() {
                return Err(format!(
                    "{prefix}/habitat/ground_cover: expected a land surface"
                ));
            }
            if habitat.ground_cover_percent > 100 {
                return Err(format!(
                    "{prefix}/habitat/ground_cover_percent: expected 0..=100"
                ));
            }
            for (field, value) in [
                ("shallow_water_tiles", habitat.shallow_water_tiles),
                ("deep_water_tiles", habitat.deep_water_tiles),
            ] {
                if value > MAX_REQUIRED_WATER_TILES {
                    return Err(format!(
                        "{prefix}/habitat/{field}: expected 0..={MAX_REQUIRED_WATER_TILES}"
                    ));
                }
            }
            if !(1..=MAX_REQUIRED_KEEPERS).contains(&species.care.required_keepers) {
                return Err(format!(
                    "{prefix}/care/required_keepers: expected 1..={MAX_REQUIRED_KEEPERS}"
                ));
            }
            let schedule = &species.care.schedule;
            if schedule.is_empty() || schedule.len() > 4 {
                return Err(format!("{prefix}/care/schedule: expected 1..=4 tasks"));
            }
            let mut categories = HashSet::new();
            for (t, task) in schedule.iter().enumerate() {
                let path = format!("{prefix}/care/schedule/{t}");
                if !categories.insert(task.category) {
                    return Err(format!("{path}/category: duplicate care category"));
                }
                if !valid_care_timing(task.first_minute_of_day, task.interval_minutes) {
                    return Err(format!(
                        "{path}: expected first_minute_of_day < 1440 and an interval of 60..=1440 minutes dividing the day"
                    ));
                }
                if !(1..=MAX_CARE_PRIORITY).contains(&task.priority) {
                    return Err(format!("{path}/priority: expected 1..={MAX_CARE_PRIORITY}"));
                }
                if !(1..=MAX_CARE_WORK_MINUTES).contains(&task.work_minutes) {
                    return Err(format!(
                        "{path}/work_minutes: expected 1..={MAX_CARE_WORK_MINUTES}"
                    ));
                }
            }
            // These are the animal presentation profiles supported by the
            // existing renderer, not a permissive generic-asset fallback.
            if ![
                "capybara", "flamingo", "zebra", "giraffe", "elephant", "penguin",
            ]
            .contains(&species.asset_ref.as_str())
            {
                return Err(format!(
                    "{prefix}/asset_ref: unsupported animal presentation"
                ));
            }
        }
        if self.species_compatibility.len() > MAX_ENTRIES {
            return Err(format!(
                "species_compatibility: expected at most {MAX_ENTRIES} entries"
            ));
        }
        let mut pairs = HashSet::new();
        for (i, pairing) in self.species_compatibility.iter().enumerate() {
            let prefix = format!("species_compatibility/{i}");
            text(&pairing.reason, &format!("{prefix}/reason"))?;
            for (s, key) in pairing.species.iter().enumerate() {
                if !keys.contains(key) {
                    return Err(format!("{prefix}/species/{s}: unknown species {key}"));
                }
            }
            let mut pair = pairing.species.clone();
            pair.sort();
            if pair[0] == pair[1] {
                return Err(format!("{prefix}/species: expected two distinct species"));
            }
            if !pairs.insert(pair) {
                return Err(format!("{prefix}/species: duplicate species pair"));
            }
        }
        let mut keys = HashSet::new();
        for (i, facility) in self.facilities.iter().enumerate() {
            let prefix = format!("facilities/{i}");
            money(
                facility.build_cost_cents,
                &format!("{prefix}/build_cost_cents"),
            )?;
            money(facility.price_cents, &format!("{prefix}/price_cents"))?;
            money(
                facility.hourly_upkeep_cents,
                &format!("{prefix}/hourly_upkeep_cents"),
            )?;
            for (dimension, value) in [
                ("width", facility.footprint.width),
                ("height", facility.footprint.height),
            ] {
                if value != 1 {
                    return Err(format!(
                        "{prefix}/footprint/{dimension}: only existing 1x1 footprints are supported"
                    ));
                }
            }
            self.references(
                &prefix,
                &facility.key,
                &facility.label,
                &facility.asset_ref,
                &facility.unlock_ref,
                &assets,
                &unlocks,
            )?;
            if !keys.insert(&facility.key) {
                return Err(format!("{prefix}/key: duplicate facilities key"));
            }
            if ["entrance", "animal_care_depot"].contains(&facility.key.as_str())
                && facility.service != facility.key
            {
                return Err(format!(
                    "{prefix}/service: fixed entity {} requires its matching service",
                    facility.key
                ));
            }
            if ["entrance", "animal_care_depot"].contains(&facility.key.as_str())
                && facility.hourly_upkeep_cents != 0
            {
                return Err(format!(
                    "{prefix}/hourly_upkeep_cents: fixed entity {} is not a built stand",
                    facility.key
                ));
            }
            if ["entrance", "animal_care_depot"].contains(&facility.service.as_str())
                && facility.key != facility.service
            {
                return Err(format!(
                    "{prefix}/key: fixed service {} requires its canonical identity",
                    facility.service
                ));
            }
            let expected_asset = match facility.service.as_str() {
                "food" => "food_stand",
                "drink" => "drink_stand",
                "toilet" => "toilet_block",
                "bench" => "bench",
                "bin" => "litter_bin",
                "entrance" => "entrance",
                "animal_care_depot" => "animal_care_depot",
                _ => return Err(format!("{prefix}/service: unsupported existing service")),
            };
            if facility.asset_ref != expected_asset {
                return Err(format!(
                    "{prefix}/asset_ref: unsupported service presentation"
                ));
            }
            let comfort = matches!(facility.service.as_str(), "toilet" | "bench" | "bin");
            if comfort && !(1..=MAX_FACILITY_CAPACITY).contains(&facility.capacity) {
                return Err(format!(
                    "{prefix}/capacity: a {} holds 1..={MAX_FACILITY_CAPACITY}",
                    facility.service
                ));
            }
            if !comfort && facility.capacity != 0 {
                return Err(format!(
                    "{prefix}/capacity: only toilets, benches and bins have a capacity"
                ));
            }
            if comfort && facility.price_cents != 0 {
                return Err(format!(
                    "{prefix}/price_cents: guest-comfort facilities are free to use"
                ));
            }
            let sells = matches!(facility.service.as_str(), "food" | "drink");
            match (&facility.stand, sells) {
                (None, true) => {
                    return Err(format!(
                        "{prefix}/stand: a {} stand needs its service model",
                        facility.service
                    ));
                }
                (Some(_), false) => {
                    return Err(format!(
                        "{prefix}/stand: only food and drink stands have a service model"
                    ));
                }
                (Some(stand), true) => {
                    for (field, value, max) in [
                        (
                            "service_minutes",
                            stand.service_minutes,
                            MAX_SERVICE_MINUTES,
                        ),
                        ("queue_capacity", stand.queue_capacity, MAX_QUEUE_CAPACITY),
                        ("max_vendors", stand.max_vendors, MAX_VENDORS),
                        ("stock_capacity", stand.stock_capacity, MAX_STOCK_CAPACITY),
                    ] {
                        if !(1..=max).contains(&value) {
                            return Err(format!("{prefix}/stand/{field}: expected 1..={max}"));
                        }
                    }
                    money(
                        stand.stock_unit_cost_cents,
                        &format!("{prefix}/stand/stock_unit_cost_cents"),
                    )?;
                }
                (None, false) => {}
            }
        }
        for fixed in ["entrance", "animal_care_depot"] {
            if !keys.contains(&fixed.to_owned()) {
                return Err(format!(
                    "facilities/{fixed}: missing fixed entity definition"
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn references(
        &self,
        prefix: &str,
        key: &str,
        label: &str,
        asset: &str,
        unlock: &str,
        assets: &HashSet<&str>,
        unlocks: &HashSet<&str>,
    ) -> Result<(), String> {
        for (field, value) in [
            ("key", key),
            ("label", label),
            ("asset_ref", asset),
            ("unlock_ref", unlock),
        ] {
            text(value, &format!("{prefix}/{field}"))?;
        }
        if !key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(format!(
                "{prefix}/key: expected stable lowercase identifier"
            ));
        }
        if !assets.contains(asset) {
            return Err(format!("{prefix}/asset_ref: unknown repository asset"));
        }
        if !unlocks.contains(unlock) {
            return Err(format!("{prefix}/unlock_ref: unknown available unlock"));
        }
        Ok(())
    }
}
