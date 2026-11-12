//! Mixed-species habitats (#108).
//!
//! A habitat's residents are the species living in it, in the order they arrived, each with an
//! owned copy of its species definition and its own welfare. A habitat keeps one species
//! unless the catalog's `species_compatibility` profiles pair every two resident species as
//! compatible: an unprofiled pair is unsupported and an incompatible pair is rejected with its
//! authored reason. These are simplified game profiles, not veterinary guidance.
//!
//! Sharing rules, decided here and in `environment.rs`:
//! - each species' social group counts only animals of that species, so one species never
//!   satisfies another's group;
//! - usable land is shared: every animal claims its own species' space;
//! - shelter and enrichment places are shared by every resident whose species needs them;
//! - feed, drinking water and cleanliness are one pool every animal draws on, and one care
//!   schedule (merged from every resident species' defaults) serves them all;
//! - each species keeps its own fence height, ground cover and water requirements.
//!
//! Every way animals arrive — adoption one at a time or in a group, transfer between habitats,
//! a loaded save — passes the same compatibility and capacity rules. The habitat's welfare,
//! appeal, upkeep, space and feed use are derived from the residents and their animals, so a
//! departure changes them at once, even while paused.
use super::{
    ActionResult, ExpenseCategory, GameState, Habitat, Species, animal, care, content,
    environment::{Assessment, Contribution},
};
use serde::{Deserialize, Serialize};

/// The most animals one adoption brings at once: a whole largest habitat.
pub(crate) const MAX_ADOPTION_GROUP: u32 = 12;
/// A species' first animals start as content as a new habitat's (100).
const ARRIVING_WELFARE: u32 = 100;
/// Feed, water and cleanliness drop every this many minutes, one point per animal.
const NEEDS_PER_HOUR: u32 = 60 / super::CARE_DECAY_INTERVAL_MINUTES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resident {
    /// An owned copy of the species definition, like the habitat's fence.
    pub species: Species,
    /// 0–100; moves one point a minute towards this species' welfare target.
    pub welfare: u32,
}

impl Habitat {
    /// How many of this habitat's animals are of species `key`.
    pub(crate) fn species_count(&self, key: &str) -> u32 {
        let count = self
            .animals
            .iter()
            .filter(|animal| animal.species == key)
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    pub(crate) fn resident_index(&self, key: &str) -> Option<usize> {
        self.residents
            .iter()
            .position(|resident| resident.species.key == key)
    }

    /// The animal-weighted mean of `value` over the residents; 100 with no animals.
    pub(crate) fn weighted(&self, value: impl Fn(&Resident) -> u32) -> u32 {
        let (sum, count) = self
            .residents
            .iter()
            .fold((0_u64, 0_u64), |(sum, count), resident| {
                let animals = u64::from(self.species_count(&resident.species.key));
                (sum + u64::from(value(resident)) * animals, count + animals)
            });
        sum.checked_div(count)
            .map_or(100, |mean| u32::try_from(mean).unwrap_or(100))
    }

    /// The habitat's welfare: its residents' welfare weighted by their animals (#108).
    pub(crate) fn welfare(&self) -> u32 {
        self.weighted(|resident| resident.welfare)
    }

    /// Resident species keys, in arrival order.
    pub(crate) fn species_keys(&self) -> Vec<String> {
        self.residents
            .iter()
            .map(|resident| resident.species.key.clone())
            .collect()
    }

    /// Resident species labels joined for messages ("Giraffe & Zebra"), or "animals".
    pub(crate) fn species_labels(&self) -> String {
        if self.residents.is_empty() {
            return "animals".to_owned();
        }
        self.residents
            .iter()
            .map(|resident| resident.species.label.as_str())
            .collect::<Vec<_>>()
            .join(" & ")
    }

    /// Every animal's species upkeep for one hour.
    pub(crate) fn animal_upkeep_cents(&self) -> i64 {
        self.residents.iter().fold(0_i64, |total, resident| {
            total.saturating_add(
                i64::from(self.species_count(&resident.species.key))
                    .saturating_mul(resident.species.hourly_upkeep_cents),
            )
        })
    }

    /// Feed, water and cleanliness points the residents use in an hour.
    fn needs_per_hour(&self) -> u32 {
        self.animal_count().saturating_mul(NEEDS_PER_HOUR)
    }

    /// One species' welfare target with this habitat as it is.
    fn species_target(&self, species: &Species, assessment: &Assessment) -> u32 {
        Self::target_of(&self.species_parts(species, assessment))
    }

    /// Animals whose species needs a place of the kind `needed` selects.
    fn sharing(&self, needed: fn(&content::HabitatRequirements) -> bool) -> u32 {
        self.residents
            .iter()
            .filter(|resident| needed(&resident.species.habitat))
            .map(|resident| self.species_count(&resident.species.key))
            .fold(0, u32::saturating_add)
    }

    /// Forgets resident species whose last animal left.
    fn prune_residents(&mut self) {
        let keys: Vec<String> = self
            .animals
            .iter()
            .map(|animal| animal.species.clone())
            .collect();
        self.residents
            .retain(|resident| keys.contains(&resident.species.key));
    }

    /// The resident views the inspector shows, one per species.
    pub(crate) fn resident_views(&self) -> Vec<ResidentView> {
        let assessment = self.assessment();
        self.residents
            .iter()
            .map(|resident| {
                let species = &resident.species;
                let parts = self.species_parts(species, &assessment);
                ResidentView {
                    species: species.key.clone(),
                    label: species.label.clone(),
                    animals: self.species_count(&species.key),
                    minimum_social_group: species.minimum_social_group,
                    welfare: resident.welfare,
                    welfare_target: Self::target_of(&parts),
                    social_score: self.social_score_of(species),
                    requirements: species.habitat.clone(),
                    welfare_parts: parts,
                }
            })
            .collect()
    }
}

/// One resident species of a habitat, summarized from its own animals (#108).
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ResidentView {
    pub species: String,
    pub label: String,
    pub animals: u32,
    pub minimum_social_group: u32,
    pub welfare: u32,
    pub welfare_target: u32,
    pub social_score: u32,
    pub requirements: content::HabitatRequirements,
    pub welfare_parts: Vec<Contribution>,
}

/// A shared place before and after an arrival: what the habitat offers and who needs it.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Share {
    pub provided: u32,
    pub needed_before: u32,
    pub needed_after: u32,
}

/// What an arrival or departure does to one resident species.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ResidentEffect {
    pub species: String,
    pub label: String,
    pub animals_before: u32,
    pub animals_after: u32,
    pub minimum_social_group: u32,
    /// Its current welfare; `None` for a species not living here yet.
    pub welfare: Option<u32>,
    pub welfare_target_before: Option<u32>,
    /// `None` when the species leaves the habitat.
    pub welfare_target_after: Option<u32>,
}

/// The effect on one habitat of animals arriving at or leaving it.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct HabitatEffect {
    pub habitat_id: u32,
    pub animals_before: u32,
    pub animals_after: u32,
    pub capacity: u32,
    pub space: Share,
    pub shelter: Share,
    pub enrichment: Share,
    /// Feed, water and cleanliness points the residents use per hour.
    pub needs_per_hour_before: u32,
    pub needs_per_hour_after: u32,
    pub upkeep_cents_per_hour_before: i64,
    pub upkeep_cents_per_hour_after: i64,
    pub residents: Vec<ResidentEffect>,
}

/// A preview of an adoption or transfer (#108): whether it is allowed, why not, and what it
/// does to the arriving animals, the residents and (for a transfer) the habitat they leave.
/// Changes nothing; the matching command plans again against the current park.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ArrivalPreview {
    pub ok: bool,
    /// The first reason it is refused, or a summary of what happens.
    pub message: String,
    /// Every reason it is refused.
    pub blockers: Vec<String>,
    /// Problems it is allowed to cause, explained.
    pub warnings: Vec<String>,
    pub species: String,
    pub label: String,
    pub count: u32,
    pub cost_cents: i64,
    /// The verdict against every other species already living in the destination.
    pub pairings: Vec<content::PairingVerdict>,
    pub destination: Option<HabitatEffect>,
    /// The habitat a transferred animal leaves.
    pub source: Option<HabitatEffect>,
}

impl ArrivalPreview {
    fn refused(message: impl Into<String>, species: &str, count: u32) -> Self {
        let message = message.into();
        Self {
            ok: false,
            blockers: vec![message.clone()],
            message,
            warnings: Vec::new(),
            species: species.to_owned(),
            label: species.to_owned(),
            count,
            cost_cents: 0,
            pairings: Vec::new(),
            destination: None,
            source: None,
        }
    }
}

/// A habitat with `count` more animals of `species` (placeholders standing at the gate).
fn with_arrivals(habitat: &Habitat, species: &Species, count: u32) -> Habitat {
    let mut after = habitat.clone();
    if after.resident_index(&species.key).is_none() {
        after.residents.push(Resident {
            species: species.clone(),
            welfare: ARRIVING_WELFARE,
        });
    }
    let position = after.gate_tile();
    for _ in 0..count {
        after.animals.push(animal::Animal {
            id: 0,
            species: species.key.clone(),
            name: String::new(),
            sex: animal::Sex::Female,
            life_stage: animal::LifeStage::Adult,
            arrived_day: 0,
            cosmetic_seed: 0,
            position,
            activity: animal::Activity::Resting { until_minute: 0 },
            energy: 0,
            decisions: 0,
            health: crate::health::Health::default(),
        });
    }
    after
}

/// Compares a habitat before and after a change of residents.
fn effect(before: &Habitat, after: &Habitat) -> HabitatEffect {
    let (assessment_before, assessment_after) = (before.assessment(), after.assessment());
    let mut species: Vec<&Species> = before
        .residents
        .iter()
        .map(|resident| &resident.species)
        .collect();
    for resident in &after.residents {
        if !species
            .iter()
            .any(|known| known.key == resident.species.key)
        {
            species.push(&resident.species);
        }
    }
    let residents = species
        .into_iter()
        .map(|species| {
            let living_before = before.resident_index(&species.key);
            let living_after = after.resident_index(&species.key);
            ResidentEffect {
                species: species.key.clone(),
                label: species.label.clone(),
                animals_before: before.species_count(&species.key),
                animals_after: after.species_count(&species.key),
                minimum_social_group: species.minimum_social_group,
                welfare: living_before.map(|index| before.residents[index].welfare),
                welfare_target_before: living_before
                    .map(|_| before.species_target(species, &assessment_before)),
                welfare_target_after: living_after
                    .map(|_| after.species_target(species, &assessment_after)),
            }
        })
        .collect();
    let share = |provided: u32, needed: fn(&Habitat) -> u32| Share {
        provided,
        needed_before: needed(before),
        needed_after: needed(after),
    };
    HabitatEffect {
        habitat_id: before.id,
        animals_before: before.animal_count(),
        animals_after: after.animal_count(),
        capacity: before.capacity(),
        space: share(assessment_after.usable_land, Habitat::required_space),
        shelter: share(assessment_after.shelter_capacity, |habitat| {
            habitat.sharing(|needs| needs.shelter)
        }),
        enrichment: share(assessment_after.enrichment_capacity, |habitat| {
            habitat.sharing(|needs| needs.enrichment)
        }),
        needs_per_hour_before: before.needs_per_hour(),
        needs_per_hour_after: after.needs_per_hour(),
        upkeep_cents_per_hour_before: before.animal_upkeep_cents(),
        upkeep_cents_per_hour_after: after.animal_upkeep_cents(),
        residents,
    }
}

/// What an arrival makes worse without being refused: a social group still too small, shared
/// space, shelter or enrichment running short, or layout the arriving species lacks.
fn warnings(after: &Habitat, species: &Species, effect: &HabitatEffect) -> Vec<String> {
    let mut warnings = Vec::new();
    let count = after.species_count(&species.key);
    if count < species.minimum_social_group {
        warnings.push(format!(
            "{} are social: {count} of at least {} together after this",
            species.label, species.minimum_social_group
        ));
    }
    if effect.space.needed_after > effect.space.provided {
        warnings.push(format!(
            "Crowded: residents need {} usable tiles; habitat #{} has {}",
            effect.space.needed_after, effect.habitat_id, effect.space.provided
        ));
    }
    for (share, place) in [
        (&effect.shelter, "shelter"),
        (&effect.enrichment, "enrichment"),
    ] {
        if share.needed_after > share.provided {
            warnings.push(format!(
                "Shared {place}: places for {} of {} animals",
                share.provided, share.needed_after
            ));
        }
    }
    let assessment = after.assessment();
    for part in after.species_parts(species, &assessment) {
        if !part.met && matches!(part.key, "ground_cover" | "shallow_water" | "deep_water") {
            warnings.push(format!(
                "{}: {}: {}",
                species.label, part.label, part.detail
            ));
        }
    }
    warnings
}

/// The warnings a departure causes in the habitat it leaves.
fn departure_warnings(after: &Habitat, species: &Species) -> Vec<String> {
    let count = after.species_count(&species.key);
    if count > 0 && count < species.minimum_social_group {
        vec![format!(
            "Habitat #{}: {count} {} stay behind, fewer than the {} they need together",
            after.id, species.label, species.minimum_social_group
        )]
    } else {
        Vec::new()
    }
}

impl GameState {
    pub(crate) fn find_habitat(&self, habitat_id: u32) -> Option<usize> {
        self.habitats
            .iter()
            .position(|habitat| habitat.id == habitat_id)
    }

    /// The habitat index and position of animal `animal_id`.
    pub(crate) fn locate_animal(&self, animal_id: u32) -> Option<(usize, usize)> {
        self.habitats
            .iter()
            .enumerate()
            .find_map(|(index, habitat)| {
                habitat
                    .animals
                    .iter()
                    .position(|animal| animal.id == animal_id)
                    .map(|at| (index, at))
            })
    }

    /// Why `count` animals of `species` cannot move into habitat `index`, if they cannot:
    /// room, compatibility with every resident species, fence height and staffing.
    pub(crate) fn arrival_blockers(
        &self,
        index: usize,
        species: &Species,
        count: u32,
    ) -> (Vec<String>, Vec<content::PairingVerdict>) {
        let habitat = &self.habitats[index];
        let mut blockers = Vec::new();
        let room = habitat.capacity().saturating_sub(habitat.animal_count());
        if room == 0 {
            blockers.push("That habitat is at capacity".to_owned());
        } else if count > room {
            blockers.push(format!(
                "Habitat #{} has room for {room} more animal{}, not {count}",
                habitat.id,
                if room == 1 { "" } else { "s" }
            ));
        }
        let pairings: Vec<content::PairingVerdict> = habitat
            .residents
            .iter()
            .filter(|resident| resident.species.key != species.key)
            .map(|resident| self.catalog.pairing(&resident.species.key, &species.key))
            .collect();
        for (resident, verdict) in habitat
            .residents
            .iter()
            .filter(|resident| resident.species.key != species.key)
            .zip(&pairings)
        {
            if !verdict.compatible {
                blockers.push(format!(
                    "{} cannot join the {} of habitat #{}: {}",
                    species.label, resident.species.label, habitat.id, verdict.reason
                ));
            }
        }
        if habitat.fence.height_cm < species.minimum_fence_height_cm {
            blockers.push(format!(
                "{} needs a fence at least {} cm high; habitat #{}'s {} is {} cm. Upgrade its fence first",
                species.label,
                species.minimum_fence_height_cm,
                habitat.id,
                habitat.fence.label.to_lowercase(),
                habitat.fence.height_cm
            ));
        }
        if let Some(problem) = self.zone_staffing_problem(habitat) {
            blockers.push(format!("Staff this habitat before adopting: {problem}"));
        }
        (blockers, pairings)
    }

    /// The species a player may adopt by key, or why not.
    fn adoptable_species(&self, key: &str) -> Result<Species, String> {
        let species = self
            .catalog
            .species
            .iter()
            .find(|species| species.key == key)
            .cloned()
            .ok_or_else(|| "Unknown species".to_owned())?;
        if !self.unlocks.species_unlocked(&species.key) {
            return Err(format!(
                "{} is not available in this scenario",
                species.label()
            ));
        }
        Ok(species)
    }

    /// Previews adopting `count` animals of `species_key` into habitat `habitat_id` (#108).
    pub(crate) fn evaluate_adoption(
        &self,
        habitat_id: u32,
        species_key: &str,
        count: u32,
    ) -> ArrivalPreview {
        let species = match self.adoptable_species(species_key) {
            Ok(species) => species,
            Err(message) => return ArrivalPreview::refused(message, species_key, count),
        };
        let Some(index) = self.find_habitat(habitat_id) else {
            return ArrivalPreview::refused("Select a habitat first", species_key, count);
        };
        if !(1..=MAX_ADOPTION_GROUP).contains(&count) {
            return ArrivalPreview::refused(
                format!("Adopt 1 to {MAX_ADOPTION_GROUP} animals at a time"),
                species_key,
                count,
            );
        }
        let (mut blockers, pairings) = self.arrival_blockers(index, &species, count);
        if self.habitats[index].quarantine {
            blockers.insert(
                0,
                format!(
                    "Habitat #{habitat_id} is set aside for quarantine; adopt into another habitat"
                ),
            );
        }
        let cost = species.purchase_cost().saturating_mul(i64::from(count));
        if blockers.is_empty() && cost > self.cash_cents {
            blockers.push("Not enough cash".to_owned());
        }
        let habitat = &self.habitats[index];
        let after = with_arrivals(habitat, &species, count);
        let destination = effect(habitat, &after);
        let message = blockers.first().cloned().unwrap_or_else(|| {
            format!("Adopt {count} {} into habitat #{habitat_id}", species.label)
        });
        ArrivalPreview {
            ok: blockers.is_empty(),
            message,
            blockers,
            warnings: warnings(&after, &species, &destination),
            species: species.key.clone(),
            label: species.label.clone(),
            count,
            cost_cents: cost,
            pairings,
            destination: Some(destination),
            source: None,
        }
    }

    /// Previews moving animal `animal_id` into habitat `habitat_id` (#108).
    pub(crate) fn evaluate_transfer(&self, animal_id: u32, habitat_id: u32) -> ArrivalPreview {
        let Some((from, at)) = self.locate_animal(animal_id) else {
            return ArrivalPreview::refused(format!("Animal #{animal_id} does not exist"), "", 1);
        };
        let source = &self.habitats[from];
        let key = source.animals[at].species.clone();
        let Some(species) = source
            .resident_index(&key)
            .map(|resident| source.residents[resident].species.clone())
        else {
            return ArrivalPreview::refused("That animal has no resident species", &key, 1);
        };
        let Some(index) = self.find_habitat(habitat_id) else {
            return ArrivalPreview::refused("Select a habitat first", &key, 1);
        };
        if index == from {
            return ArrivalPreview::refused(
                format!("Animal #{animal_id} already lives in habitat #{habitat_id}"),
                &key,
                1,
            );
        }
        let (mut blockers, pairings) = self.arrival_blockers(index, &species, 1);
        // Quarantine has its own way in and out (#109).
        if self.habitats[index].quarantine {
            blockers.insert(
                0,
                format!(
                    "Habitat #{habitat_id} is set aside for quarantine; use Move to quarantine for an unwell animal"
                ),
            );
        }
        if source.quarantine && source.animals[at].health.is_ill() {
            blockers.insert(
                0,
                format!(
                    "{} stays in quarantine until it has recovered",
                    source.animals[at].name
                ),
            );
        }
        let habitat = &self.habitats[index];
        let after = with_arrivals(habitat, &species, 1);
        let destination = effect(habitat, &after);
        let mut source_after = source.clone();
        source_after.animals.remove(at);
        source_after.prune_residents();
        let mut warnings = warnings(&after, &species, &destination);
        warnings.extend(departure_warnings(&source_after, &species));
        let name = &source.animals[at].name;
        let message = blockers.first().cloned().unwrap_or_else(|| {
            format!(
                "Move {name} (#{animal_id}) from habitat #{} to habitat #{habitat_id}",
                source.id
            )
        });
        ArrivalPreview {
            ok: blockers.is_empty(),
            message,
            blockers,
            warnings,
            species: species.key.clone(),
            label: species.label.clone(),
            count: 1,
            cost_cents: 0,
            pairings,
            destination: Some(destination),
            source: Some(effect(source, &source_after)),
        }
    }

    /// Makes `species` a resident of habitat `index` if it is not one yet, and merges its
    /// default care into the habitat's schedule: the larger crew and any task category the
    /// schedule lacks. Player-customized tasks are never changed.
    fn welcome_species(&mut self, index: usize, species: &Species, welfare: u32) {
        let now = self.absolute_minute();
        let habitat = &mut self.habitats[index];
        match habitat.care_schedule.as_mut() {
            None => {
                habitat.care_schedule = Some(care::CareSchedule::instantiate(&species.care, now));
            }
            Some(schedule) => {
                schedule.required_keepers =
                    schedule.required_keepers.max(species.care.required_keepers);
                for task in care::CareSchedule::instantiate(&species.care, now).tasks {
                    if !schedule
                        .tasks
                        .iter()
                        .any(|existing| existing.category == task.category)
                    {
                        schedule.tasks.push(task);
                    }
                }
            }
        }
        match habitat.resident_index(&species.key) {
            // Re-adopting refreshes the owned definition from the catalog, as before #108.
            Some(resident) => habitat.residents[resident].species = species.clone(),
            None => habitat.residents.push(Resident {
                species: species.clone(),
                welfare,
            }),
        }
    }

    /// Adopts `count` animals of one species into a habitat as one transaction (#108): every
    /// animal passes the same room, compatibility, fence and staffing rules together, and the
    /// whole group is paid for at once or not at all.
    pub(crate) fn adopt_group(
        &mut self,
        habitat_id: u32,
        species_key: &str,
        count: u32,
    ) -> ActionResult {
        let preview = self.evaluate_adoption(habitat_id, species_key, count);
        if !preview.ok {
            return ActionResult::error(preview.message);
        }
        let species = self
            .adoptable_species(species_key)
            .expect("the preview found the species");
        let index = self
            .find_habitat(habitat_id)
            .expect("the preview found the habitat");
        if let Err(message) = self.spend(preview.cost_cents, ExpenseCategory::AnimalPurchase) {
            return ActionResult::error(message);
        }
        self.welcome_species(index, &species, ARRIVING_WELFARE);
        let mut arrivals = Vec::new();
        for _ in 0..count {
            let id = self.add_animal(index, &species.key);
            let name = self.habitats[index]
                .animals
                .last()
                .map_or_else(String::new, |animal| animal.name.clone());
            arrivals.push(format!("{name} (#{id})"));
        }
        ActionResult::ok(if count == 1 {
            format!(
                "{} adopted into habitat #{habitat_id}: {}",
                species.label(),
                arrivals.join(", ")
            )
        } else {
            format!(
                "{count} {} adopted into habitat #{habitat_id}: {}",
                species.label(),
                arrivals.join(", ")
            )
        })
    }

    /// Moves one animal, with its identity, name and needs, into another habitat (#108). The
    /// move passes the same rules as an adoption; a species new to the destination brings the
    /// welfare it had where it came from.
    pub(crate) fn transfer_animal(&mut self, animal_id: u32, habitat_id: u32) -> ActionResult {
        let preview = self.evaluate_transfer(animal_id, habitat_id);
        if !preview.ok {
            return ActionResult::error(preview.message);
        }
        let (from, _) = self
            .locate_animal(animal_id)
            .expect("the preview found the animal");
        let source_id = self.habitats[from].id;
        let name = self.relocate_animal(animal_id, habitat_id);
        // A recovered patient moved elsewhere has left quarantine for good (#109).
        if let Some((index, at)) = self.locate_animal(animal_id) {
            self.habitats[index].animals[at].health.quarantine_home = None;
        }
        self.emit("animal_transferred", Some(animal_id));
        ActionResult::ok(format!(
            "{name} (#{animal_id}) moved from habitat #{source_id} to habitat #{habitat_id}"
        ))
    }

    /// Moves one animal, with its identity, name, needs and health, into habitat
    /// `habitat_id`; the caller has checked the move. Returns its name.
    pub(crate) fn relocate_animal(&mut self, animal_id: u32, habitat_id: u32) -> String {
        let (from, at) = self
            .locate_animal(animal_id)
            .expect("the caller found the animal");
        let index = self
            .find_habitat(habitat_id)
            .expect("the caller found the habitat");
        let source = &self.habitats[from];
        let resident = source
            .resident_index(&source.animals[at].species)
            .expect("the caller found the resident species");
        let (species, welfare) = (
            source.residents[resident].species.clone(),
            source.residents[resident].welfare,
        );
        let mut moving = self.habitats[from].animals.remove(at);
        self.habitats[from].prune_residents();
        self.welcome_species(index, &species, welfare);
        let now = self.absolute_minute();
        let minute_of_day = self.minute_of_day;
        let habitat = &mut self.habitats[index];
        moving.position = animal::arrival_tile(
            habitat.roam_area().pool(),
            habitat.id,
            habitat.animal_count(),
            minute_of_day,
        );
        moving.activity = animal::Activity::Resting { until_minute: now };
        let name = moving.name.clone();
        habitat.animals.push(moving);
        // A treatment follows its patient at once (#109).
        self.reconcile_treatment_jobs(now);
        name
    }

    /// Sends one animal to a partner zoo (#108): it leaves the park for good, with no refund,
    /// and its habitat's residents, welfare, upkeep and shared needs follow at once.
    pub(crate) fn rehome_animal(&mut self, animal_id: u32) -> ActionResult {
        let Some((from, at)) = self.locate_animal(animal_id) else {
            return ActionResult::error(format!("Animal #{animal_id} does not exist"));
        };
        let habitat = &mut self.habitats[from];
        let leaving = habitat.animals.remove(at);
        habitat.prune_residents();
        let habitat_id = habitat.id;
        // Its treatment, if any, is withdrawn at once (#109).
        self.reconcile_treatment_jobs(self.absolute_minute());
        self.emit("animal_departed", Some(animal_id));
        ActionResult::ok(format!(
            "{} (#{animal_id}) left habitat #{habitat_id} for a partner zoo",
            leaving.name
        ))
    }

    /// Why the residents are inconsistent, if they are (#108): every resident species is
    /// distinct, matches the content and has animals; every animal's species is a resident;
    /// every two resident species are a compatible pair; welfare is a percentage.
    pub(crate) fn resident_violation(&self) -> Option<String> {
        for habitat in &self.habitats {
            let id = habitat.id;
            for (index, resident) in habitat.residents.iter().enumerate() {
                let species = &resident.species;
                if !self.catalog.species.contains(species) {
                    return Some(format!(
                        "habitat {id} houses species {} that does not match the content",
                        species.key
                    ));
                }
                if habitat.residents[..index]
                    .iter()
                    .any(|other| other.species.key == species.key)
                {
                    return Some(format!("habitat {id} lists species {} twice", species.key));
                }
                if habitat.species_count(&species.key) == 0 {
                    return Some(format!(
                        "habitat {id} lists species {} without animals",
                        species.key
                    ));
                }
                if resident.welfare > 100 {
                    return Some(format!("habitat {id} welfare is above 100"));
                }
                for other in &habitat.residents[..index] {
                    let verdict = self.catalog.pairing(&other.species.key, &species.key);
                    if !verdict.compatible {
                        return Some(format!(
                            "habitat {id} mixes {} and {}: {}",
                            other.species.key, species.key, verdict.reason
                        ));
                    }
                }
            }
            if let Some(animal) = habitat
                .animals
                .iter()
                .find(|animal| habitat.resident_index(&animal.species).is_none())
            {
                return Some(format!(
                    "animal {} is a {} but habitat {id} keeps no such species",
                    animal.id, animal.species
                ));
            }
        }
        None
    }
}
