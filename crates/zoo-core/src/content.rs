//! Validated, immutable content owned by a park. Presentation references are IDs
//! from Zoo's existing asset manifest; they never select simulation rules.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 128;
const MAX_STRING_BYTES: usize = 256;
pub(crate) const MAX_CENTS: i64 = 9_007_199_254_740_991;
// A park has WIDTH*HEIGHT tiles and habitat capacity is twelve. Bound even
// a conservative whole-park appeal sum before any arithmetic is performed.
const MAX_RULE_VALUE: u32 = u32::MAX / (super::WIDTH * super::HEIGHT * 12);
pub(crate) const SCHEMA_VERSION: u32 = 2;
pub(crate) const DEFAULT_DOCUMENT: &str = include_str!("../content/catalog.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalog {
    pub schema_version: u32,
    pub ruleset_id: String,
    pub balance: Balance,
    pub species: Vec<Species>,
    pub facilities: Vec<Facility>,
    pub unlocks: Vec<Unlock>,
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
    /// Operating cost charged every in-game hour for each animal of this species.
    pub hourly_upkeep_cents: i64,
    pub asset_ref: String,
    pub unlock_ref: String,
    pub care: CareProfile,
}

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
    pub footprint: Footprint,
}

/// Park-wide prices, operating costs and wages. Species and facility costs live on their
/// own definitions; everything here applies to the whole park.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Balance {
    pub admission_price_cents: i64,
    pub construction: ConstructionCosts,
    pub habitat_care: HabitatCareCosts,
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
    pub fence_segment_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HabitatCareCosts {
    pub water_refill_cents: i64,
    pub cleaning_cents: i64,
    pub shelter_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StaffCosts {
    pub keeper: StaffRole,
    pub janitor: StaffRole,
    pub mechanic: StaffRole,
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
    pub fence_segment_hourly_cents: i64,
}

impl Balance {
    fn validate(&self) -> Result<(), String> {
        let build = &self.construction;
        let care = &self.habitat_care;
        let staff = &self.staff;
        let upkeep = &self.upkeep;
        for (path, value) in [
            ("admission_price_cents", self.admission_price_cents),
            ("construction/path_tile_cents", build.path_tile_cents),
            ("construction/habitat_base_cents", build.habitat_base_cents),
            ("construction/habitat_tile_cents", build.habitat_tile_cents),
            (
                "construction/fence_segment_cents",
                build.fence_segment_cents,
            ),
            ("habitat_care/water_refill_cents", care.water_refill_cents),
            ("habitat_care/cleaning_cents", care.cleaning_cents),
            ("habitat_care/shelter_cents", care.shelter_cents),
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
            ("upkeep/habitat_hourly_cents", upkeep.habitat_hourly_cents),
            (
                "upkeep/fence_segment_hourly_cents",
                upkeep.fence_segment_hourly_cents,
            ),
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
    pub fn parse(document: &str) -> Result<Self, String> {
        if document.len() > MAX_DOCUMENT_BYTES {
            return Err(format!("catalog: exceeds {MAX_DOCUMENT_BYTES} bytes"));
        }
        // Check the version before the full schema, so a catalog written for another version
        // (such as one embedded in an older save) reports that instead of a missing field.
        #[derive(Deserialize)]
        struct Version {
            schema_version: u32,
        }
        let version: Version = serde_json::from_str(document)
            .map_err(|error| format!("catalog/schema_version: {error}"))?;
        if version.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "schema_version: only version {SCHEMA_VERSION} is supported"
            ));
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
                "entrance" => "entrance",
                "animal_care_depot" => "animal_care_depot",
                _ => return Err(format!("{prefix}/service: unsupported existing service")),
            };
            if facility.asset_ref != expected_asset {
                return Err(format!(
                    "{prefix}/asset_ref: unsupported service presentation"
                ));
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
