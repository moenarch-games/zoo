//! Individual animals (#96).
//!
//! Every animal is a persistent record owned by the habitat it lives in: a park-wide stable ID
//! (never reused, allocated from `GameState::next_animal_id`), its species, a player-editable
//! name, sex and life stage, the day it arrived, a cosmetic seed, its current tile, its current
//! activity and its individual needs. A habitat's animal count is the length of its roster;
//! there is no competing count.
//!
//! Movement and activity are game rules decided here, deterministically, every
//! [`STEP_INTERVAL_MINUTES`] simulated minutes. An animal walks one tile at a time inside its
//! habitat's tiles towards a target chosen from the roaming tiles clear of the fence, then rests
//! there; walking tires it and resting restores it. Choices hash the animal's own seed and
//! decision counter, never a shared stream, so adding, removing or reordering other animals
//! never changes an animal's identity, seed or decisions. Rendering interpolates these states;
//! it never supplies movement truth.
//!
//! Lifecycle changes (arrival, renaming, departure) go through the `GameState` methods in this
//! module and emit `animal_*` domain events, the seam later health, transfer and breeding work
//! builds on.
use super::{GameState, Habitat, Position, environment::RoamArea, shape};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// How often animals take one movement/activity step.
pub(crate) const STEP_INTERVAL_MINUTES: u32 = 2;
pub(crate) const MAX_NAME_CHARS: usize = 24;
const MAX_ENERGY: u32 = 100;
/// Below this an arriving animal takes a long rest instead of a short pause.
const TIRED_ENERGY: u32 = 40;
const WALK_ENERGY_COST: u32 = 1;
const REST_ENERGY_GAIN: u32 = 4;
const TIRED_REST_MINUTES: u64 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sex {
    Female,
    Male,
}

/// Every adopted animal arrives as an adult; juveniles arrive with breeding work later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifeStage {
    Adult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Activity {
    Walking { target: Position },
    Resting { until_minute: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Animal {
    pub id: u32,
    /// The species content key; always the habitat's species.
    pub species: String,
    pub name: String,
    pub sex: Sex,
    pub life_stage: LifeStage,
    pub arrived_day: u32,
    /// Varies presentation (animation phase, offset within the tile) and seeds decisions.
    pub cosmetic_seed: u32,
    pub position: Position,
    pub activity: Activity,
    /// 0–100; walking spends it, resting restores it.
    pub energy: u32,
    /// How many targets this animal has chosen; with the seed it decides the next one.
    pub decisions: u32,
}

/// A stateless 32-bit mix (lowbias32): the same inputs always give the same output.
pub(crate) fn mix(seed: u32, salt: u32) -> u32 {
    let mut value = seed ^ salt.wrapping_mul(0x9E37_79B9);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^= value >> 16;
    value
}

const NAMES: [&str; 32] = [
    "Ada", "Basil", "Clover", "Dot", "Ember", "Fennel", "Ginger", "Hazel", "Indigo", "Juniper",
    "Kiwi", "Luna", "Maple", "Nutmeg", "Olive", "Pip", "Quill", "Rosie", "Sage", "Tansy", "Umber",
    "Violet", "Willow", "Xena", "Yarrow", "Ziggy", "Bramble", "Cocoa", "Dune", "Fig", "Moss",
    "Pebble",
];

/// The seed for a newly arrived animal: a mix of its ID, so it never depends on its neighbours.
pub(crate) fn seed_for(id: u32) -> u32 {
    mix(id, 0x5EED)
}

pub(crate) fn default_name(seed: u32) -> String {
    NAMES[(mix(seed, 1) as usize) % NAMES.len()].to_owned()
}

pub(crate) fn sex_for(seed: u32) -> Sex {
    if mix(seed, 2).is_multiple_of(2) {
        Sex::Female
    } else {
        Sex::Male
    }
}

/// A trimmed, non-empty name of at most [`MAX_NAME_CHARS`] printable characters.
pub(crate) fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Animal names cannot be empty".to_owned());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "Animal names have at most {MAX_NAME_CHARS} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("Animal names cannot contain control characters".to_owned());
    }
    Ok(name.to_owned())
}

/// The tiles animals pick targets from: those clear of the fence, or all when there are none.
pub(crate) fn roam_tiles(tiles: &[Position]) -> Vec<Position> {
    let inner = shape::inner_tiles(tiles);
    if inner.is_empty() {
        tiles.to_vec()
    } else {
        inner
    }
}

/// Where the `slot`-th animal of habitat `habitat_id` arrives at `minute_of_day`: the roaming
/// tile the count-based model (schema 6 and older) placed that slot at, so arrivals and
/// migrated parks start exactly where animals used to stand.
/// `roam` is the habitat's target tiles (see [`roam_tiles`] and the environment's roam area).
pub(crate) fn arrival_tile(
    roam: &[Position],
    habitat_id: u32,
    slot: u32,
    minute_of_day: u32,
) -> Position {
    let Some(count) = u32::try_from(roam.len()).ok().filter(|count| *count > 0) else {
        return Position { x: 0, y: 0 };
    };
    let index = (minute_of_day / 2)
        .saturating_add(slot.saturating_mul(3))
        .saturating_add(habitat_id.saturating_mul(5))
        % count;
    roam[index as usize]
}

impl Animal {
    /// One movement/activity step at `now` inside the habitat's roam area.
    fn step(&mut self, area: &RoamArea, now: u64) {
        match self.activity {
            Activity::Resting { until_minute } => {
                self.energy = self.energy.saturating_add(REST_ENERGY_GAIN).min(MAX_ENERGY);
                if now >= until_minute {
                    self.choose_target(area);
                }
            }
            Activity::Walking { target } => {
                if self.position == target {
                    let minutes = if self.energy < TIRED_ENERGY {
                        TIRED_REST_MINUTES
                    } else {
                        u64::from(4 + mix(self.cosmetic_seed, self.decisions) % 8 * 2)
                    };
                    self.activity = Activity::Resting {
                        until_minute: now.saturating_add(minutes),
                    };
                } else {
                    self.position = area.step_towards(self.position, target);
                    self.energy = self.energy.saturating_sub(WALK_ENERGY_COST);
                }
            }
        }
    }

    fn choose_target(&mut self, area: &RoamArea) {
        let roam = area.pool();
        let target = roam
            .get(
                (mix(self.cosmetic_seed, self.decisions.wrapping_add(1000)) as usize)
                    % roam.len().max(1),
            )
            .copied()
            .unwrap_or(self.position);
        self.decisions = self.decisions.wrapping_add(1);
        self.activity = Activity::Walking { target };
    }

    /// Keeps the animal and its target on ground it can reach after a reshape or terrain edit.
    pub(crate) fn settle_into(&mut self, area: &RoamArea) {
        if !area.contains(self.position) {
            self.position = area.nearest(self.position);
        }
        if let Activity::Walking { target } = self.activity
            && !area.contains(target)
        {
            self.activity = Activity::Walking {
                target: area.nearest(target),
            };
        }
    }

    pub(crate) fn activity_label(&self) -> &'static str {
        match self.activity {
            Activity::Walking { .. } => "walking",
            Activity::Resting { .. } => "resting",
        }
    }
}

impl Habitat {
    pub(crate) fn animal_count(&self) -> u32 {
        u32::try_from(self.animals.len()).unwrap_or(u32::MAX)
    }

    pub(crate) fn stocked(&self) -> bool {
        !self.animals.is_empty()
    }
}

impl GameState {
    /// A newly adopted adult of the habitat's species, about to choose where to walk.
    pub(crate) fn add_animal(&mut self, habitat_index: usize, species: &str) -> u32 {
        let id = self.next_animal_id;
        self.next_animal_id = self.next_animal_id.saturating_add(1);
        let seed = seed_for(id);
        let now = self.absolute_minute();
        let day = self.day;
        let minute_of_day = self.minute_of_day;
        let habitat = &mut self.habitats[habitat_index];
        let position = arrival_tile(
            habitat.roam_area().pool(),
            habitat.id,
            habitat.animal_count(),
            minute_of_day,
        );
        habitat.animals.push(Animal {
            id,
            species: species.to_owned(),
            name: default_name(seed),
            sex: sex_for(seed),
            life_stage: LifeStage::Adult,
            arrived_day: day,
            cosmetic_seed: seed,
            position,
            activity: Activity::Resting { until_minute: now },
            energy: MAX_ENERGY,
            decisions: 0,
        });
        self.emit("animal_arrived", Some(id));
        id
    }

    pub(crate) fn rename_animal(&mut self, animal_id: u32, name: &str) -> super::ActionResult {
        let name = match validate_name(name) {
            Ok(name) => name,
            Err(message) => return super::ActionResult::error(message),
        };
        let Some(animal) = self
            .habitats
            .iter_mut()
            .flat_map(|habitat| habitat.animals.iter_mut())
            .find(|animal| animal.id == animal_id)
        else {
            return super::ActionResult::error(format!("Animal #{animal_id} does not exist"));
        };
        if animal.name == name {
            return super::ActionResult::ok(format!("Animal #{animal_id} is already {name}"));
        }
        animal.name.clone_from(&name);
        self.emit("animal_renamed", Some(animal_id));
        super::ActionResult::ok(format!("Animal #{animal_id} is now called {name}"))
    }

    /// Emits a departure for every animal of a habitat that is about to be removed.
    pub(crate) fn animals_depart(&mut self, habitat_index: usize) {
        let ids: Vec<u32> = self.habitats[habitat_index]
            .animals
            .iter()
            .map(|animal| animal.id)
            .collect();
        for id in ids {
            self.emit("animal_departed", Some(id));
        }
    }

    pub(crate) fn advance_animals(&mut self) {
        let now = self.absolute_minute();
        for habitat in &mut self.habitats {
            if habitat.animals.is_empty() {
                continue;
            }
            let area = habitat.roam_area();
            for animal in &mut habitat.animals {
                animal.step(&area, now);
            }
        }
    }

    /// Why the animal records are inconsistent, if they are: IDs unique and allocated, every
    /// animal of its habitat's species and inside its tiles, names valid, needs in range.
    pub(crate) fn animal_violation(&self) -> Option<String> {
        let mut ids = HashSet::new();
        for habitat in &self.habitats {
            if habitat.animal_count() > habitat.capacity() {
                return Some(format!(
                    "habitat {} keeps {} animals above its capacity {}",
                    habitat.id,
                    habitat.animal_count(),
                    habitat.capacity()
                ));
            }
            for animal in &habitat.animals {
                if animal.id == 0 || animal.id >= self.next_animal_id || !ids.insert(animal.id) {
                    return Some(format!(
                        "animal id {} is duplicated or not yet allocated (next {})",
                        animal.id, self.next_animal_id
                    ));
                }
                if habitat.species.as_ref().map(|s| s.key.as_str()) != Some(animal.species.as_str())
                {
                    return Some(format!(
                        "animal {} is a {} but habitat {} keeps another species",
                        animal.id, animal.species, habitat.id
                    ));
                }
                if !habitat.contains(animal.position) {
                    return Some(format!(
                        "animal {} stands outside habitat {}",
                        animal.id, habitat.id
                    ));
                }
                if let Activity::Walking { target } = animal.activity
                    && !habitat.contains(target)
                {
                    return Some(format!(
                        "animal {} walks towards a tile outside habitat {}",
                        animal.id, habitat.id
                    ));
                }
                if animal.energy > MAX_ENERGY {
                    return Some(format!("animal {} energy is above {MAX_ENERGY}", animal.id));
                }
                if validate_name(&animal.name).ok().as_deref() != Some(animal.name.as_str()) {
                    return Some(format!("animal {} has an invalid name", animal.id));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_the_enclosure_around_a_notch() {
        // An L: the corner tile (2,0) is missing, so (1,0) → (2,1) must go through (1,1).
        let tiles = vec![
            Position { x: 0, y: 0 },
            Position { x: 1, y: 0 },
            Position { x: 0, y: 1 },
            Position { x: 1, y: 1 },
            Position { x: 2, y: 1 },
        ];
        let area = super::super::environment::roam_area(
            &tiles,
            &super::super::environment::Environment::default(),
            Position { x: 0, y: 0 },
        );
        let next = area.step_towards(Position { x: 1, y: 0 }, Position { x: 2, y: 1 });
        assert_eq!(next, Position { x: 1, y: 1 });
    }

    #[test]
    fn names_validate() {
        assert_eq!(validate_name("  Pip ").as_deref(), Ok("Pip"));
        assert!(validate_name("").is_err());
        assert!(validate_name(&"x".repeat(25)).is_err());
        assert!(validate_name("a\nb").is_err());
    }
}
