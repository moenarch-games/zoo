//! Utilities, habitat climate control and service outages (#113).
//!
//! Zoo-management scale, not a network simulator: a utility plant is a one-tile facility with a
//! fixed capacity of one utility (pool water or climate control). It serves a habitat when a
//! staff-walkable path joins one of its service tiles to the habitat's service gate within the
//! plant's coverage; there are no pipes or cables to route. Species content says how many units
//! each animal draws, so demand comes from the residents and nothing else: decorative scenery,
//! furnishings, the renderer's lighting and an unconnected or failed plant supply nothing.
//!
//! Allocation is recomputed from authoritative state after every command and simulated minute.
//! For each utility, habitats are served in a fixed order (their priority, high first, then
//! habitat ID); each draws whole units from the plants it reaches, nearest first and then by
//! plant ID, until its demand is met or no reachable capacity is left. A plant never hands out
//! more than its capacity, and every shortfall is stored with its reason (none built, not
//! connected, failed equipment, overloaded) and shown in alerts and inspectors.
//!
//! Plants wear slowly and reuse the stand maintenance pipeline: a worn plant raises one
//! maintenance task for mechanics, a failed one supplies nothing until it is repaired or
//! replaced. Consequences are deliberately gentle (Decision 6.1.12): a shortage is a warning for
//! its first hour, then a bounded welfare part for the affected species only; supply restored
//! clears it at once and welfare recovers as usual. Nothing dies or cascades.
use super::{
    ActionResult, ExpenseCategory, GameState, Habitat, Mover, Position,
    alerts::{AlertAction, AlertCode, Severity, Subject},
    construction::dollars,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub(crate) const MAX_UTILITY_CAPACITY: u32 = 64;
pub(crate) const MAX_UTILITY_COVERAGE: u32 = 64;
pub(crate) const MAX_UTILITY_DEMAND: u32 = 8;
/// A shortage is only a warning for this long; afterwards it lowers the welfare target.
pub(crate) const SHORTAGE_GRACE_MINUTES: u32 = 60;
/// Shortage minutes are counted up to a day; the effect is bounded either way.
const MAX_SHORTAGE_MINUTES: u32 = 24 * 60;
/// Condition a plant loses every maintenance interval: a quarter of a stand's wear.
pub(crate) const UTILITY_WEAR_PER_INTERVAL: u32 = 2;
/// The welfare part of a degraded habitat scores this much with no supply at all.
const DEGRADED_FLOOR: u32 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UtilityKind {
    Water,
    Climate,
}

impl UtilityKind {
    pub(crate) const ALL: [Self; 2] = [Self::Water, Self::Climate];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Water => "water",
            Self::Climate => "climate",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Water => "Pool water",
            Self::Climate => "Climate control",
        }
    }

    fn plant(self) -> &'static str {
        match self {
            Self::Water => "water pump",
            Self::Climate => "climate plant",
        }
    }
}

/// A plant's content definition: what it supplies, how many units and how many path tiles away.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UtilityService {
    pub kind: UtilityKind,
    pub capacity: u32,
    /// Path tiles from the plant to a habitat's gate it still serves, counting both ends.
    pub coverage_tiles: u32,
}

/// Units of each utility one animal of a species draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UtilityDemand {
    #[serde(default)]
    pub water: u32,
    #[serde(default)]
    pub climate: u32,
}

impl UtilityDemand {
    pub(crate) fn is_zero(&self) -> bool {
        self.water == 0 && self.climate == 0
    }

    pub(crate) fn of(self, kind: UtilityKind) -> u32 {
        match kind {
            UtilityKind::Water => self.water,
            UtilityKind::Climate => self.climate,
        }
    }
}

/// Which habitats are served first when capacity is short; ties go to the lower habitat ID.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UtilityPriority {
    High,
    #[default]
    Normal,
    Low,
}

impl UtilityPriority {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "high" => Some(Self::High),
            "normal" => Some(Self::Normal),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }
}

/// Why a habitat gets less than it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShortageReason {
    /// The park has no plant of this kind.
    NoneBuilt,
    /// Plants exist, but no path joins any of them to the gate within their coverage.
    Unconnected,
    /// A plant the habitat reaches has failed and supplies nothing.
    Failed,
    /// Every reachable working plant's capacity is taken by habitats served before it.
    Overloaded,
}

/// Units one habitat draws from one plant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Draw {
    pub concession_id: u32,
    pub units: u32,
}

/// A habitat's latest allocation of one utility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Supply {
    pub demand: u32,
    pub supplied: u32,
    /// Simulated minutes the habitat has been short without a break, capped at a day.
    pub shortage_minutes: u32,
    /// Its place in this utility's service order, from 1; 0 without demand.
    pub rank: u32,
    pub reason: Option<ShortageReason>,
    pub draws: Vec<Draw>,
    /// The plants of this kind it reaches, nearest first, working or not.
    pub reachable: Vec<u32>,
}

impl Supply {
    pub(crate) fn short(&self) -> bool {
        self.supplied < self.demand
    }

    pub(crate) fn degraded(&self) -> bool {
        self.short() && self.shortage_minutes >= SHORTAGE_GRACE_MINUTES
    }
}

/// A habitat's utility priority and supply (#113).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HabitatUtilities {
    pub priority: UtilityPriority,
    pub water: Supply,
    pub climate: Supply,
}

impl HabitatUtilities {
    pub(crate) fn supply(&self, kind: UtilityKind) -> &Supply {
        match kind {
            UtilityKind::Water => &self.water,
            UtilityKind::Climate => &self.climate,
        }
    }

    fn supply_mut(&mut self, kind: UtilityKind) -> &mut Supply {
        match kind {
            UtilityKind::Water => &mut self.water,
            UtilityKind::Climate => &mut self.climate,
        }
    }
}

impl Habitat {
    /// Units of `kind` the residents draw: each animal its species' amount.
    pub(crate) fn utility_demand(&self, kind: UtilityKind) -> u32 {
        self.residents.iter().fold(0, |total, resident| {
            total.saturating_add(
                self.species_count(&resident.species.key)
                    .saturating_mul(resident.species.utilities.of(kind)),
            )
        })
    }

    /// The welfare part a species shows while a utility it needs is degraded; `None` while
    /// supplied, in its grace hour or not needed, so supplied habitats keep their welfare.
    pub(crate) fn utility_part(
        &self,
        species: &super::Species,
        kind: UtilityKind,
    ) -> Option<(u32, String)> {
        let supply = self.utilities.supply(kind);
        if species.utilities.of(kind) == 0 || !supply.degraded() {
            return None;
        }
        let score =
            DEGRADED_FLOOR + (100 - DEGRADED_FLOOR) * supply.supplied / supply.demand.max(1);
        Some((
            score,
            format!(
                "{} of {} units for {} hours: {}",
                supply.supplied,
                supply.demand,
                supply.shortage_minutes / 60,
                supply.reason.map_or("short", reason_text)
            ),
        ))
    }
}

fn reason_text(reason: ShortageReason) -> &'static str {
    match reason {
        ShortageReason::NoneBuilt => "no plant is built",
        ShortageReason::Unconnected => "no plant is connected by path",
        ShortageReason::Failed => "a connected plant has failed",
        ShortageReason::Overloaded => "connected plants are overloaded",
    }
}

/// One plant's inputs to the allocation.
struct Plant {
    id: u32,
    kind: UtilityKind,
    working: bool,
    capacity: u32,
    /// Path distance from the plant to every tile, within its coverage.
    distances: Vec<Option<u32>>,
}

impl GameState {
    fn utility_plants(&self) -> Vec<Plant> {
        self.concessions
            .iter()
            .filter_map(|stand| {
                let service = stand.definition.utility?;
                Some(Plant {
                    id: stand.id,
                    kind: service.kind,
                    working: stand.condition > super::MAINTENANCE_FAILURE_THRESHOLD,
                    capacity: service.capacity,
                    distances: self.utility_distances(stand.id, service.coverage_tiles),
                })
            })
            .collect()
    }

    /// Staff-walkable path distance from the plant: 1 on its service tiles, up to `coverage`.
    pub(crate) fn utility_distances(&self, concession_id: u32, coverage: u32) -> Vec<Option<u32>> {
        let mut distance = vec![None; self.tiles.len()];
        let mut queue = VecDeque::new();
        for start in self.concession_service_tiles(concession_id) {
            if let Some(index) = self.index(start.x, start.y)
                && distance[index].is_none()
            {
                distance[index] = Some(1);
                queue.push_back(start);
            }
        }
        while let Some(current) = queue.pop_front() {
            let Some(here) = self
                .index(current.x, current.y)
                .and_then(|index| distance[index])
            else {
                continue;
            };
            if here >= coverage {
                continue;
            }
            for next in self.neighbors(current) {
                if let Some(index) = self.index(next.x, next.y)
                    && distance[index].is_none()
                    && self.walkable(Mover::Staff, next)
                {
                    distance[index] = Some(here + 1);
                    queue.push_back(next);
                }
            }
        }
        distance
    }

    /// Recomputes every habitat's allocation from the current park; shortage time is kept for
    /// habitats still short and cleared for the rest.
    pub(crate) fn refresh_utilities(&mut self) {
        let plants = self.utility_plants();
        let gates: Vec<Option<Position>> = self
            .habitats
            .iter()
            .map(|habitat| self.gate_access_tile(habitat.service_gate))
            .collect();
        for kind in UtilityKind::ALL {
            let mut remaining: Vec<u32> = plants
                .iter()
                .map(|plant| if plant.working { plant.capacity } else { 0 })
                .collect();
            let any_built = plants.iter().any(|plant| plant.kind == kind);
            let mut order: Vec<usize> = (0..self.habitats.len())
                .filter(|&index| self.habitats[index].utility_demand(kind) > 0)
                .collect();
            order.sort_by_key(|&index| {
                let habitat = &self.habitats[index];
                (habitat.utilities.priority, habitat.id)
            });
            let mut supplies: Vec<Supply> = vec![Supply::default(); self.habitats.len()];
            for (rank, &index) in order.iter().enumerate() {
                let demand = self.habitats[index].utility_demand(kind);
                let mut reach: Vec<(u32, u32, usize)> = gates[index]
                    .and_then(|gate| self.index(gate.x, gate.y))
                    .map(|tile| {
                        plants
                            .iter()
                            .enumerate()
                            .filter(|(_, plant)| plant.kind == kind)
                            .filter_map(|(at, plant)| Some((plant.distances[tile]?, plant.id, at)))
                            .collect()
                    })
                    .unwrap_or_default();
                reach.sort_unstable();
                let mut supply = Supply {
                    demand,
                    rank: u32::try_from(rank + 1).unwrap_or(u32::MAX),
                    reachable: reach.iter().map(|&(_, id, _)| id).collect(),
                    ..Supply::default()
                };
                for &(_, id, at) in &reach {
                    let units = remaining[at].min(demand - supply.supplied);
                    if units > 0 {
                        remaining[at] -= units;
                        supply.supplied += units;
                        supply.draws.push(Draw {
                            concession_id: id,
                            units,
                        });
                    }
                }
                if supply.short() {
                    supply.reason = Some(if !any_built {
                        ShortageReason::NoneBuilt
                    } else if reach.is_empty() {
                        ShortageReason::Unconnected
                    } else if reach.iter().any(|&(_, _, at)| !plants[at].working) {
                        ShortageReason::Failed
                    } else {
                        ShortageReason::Overloaded
                    });
                }
                supplies[index] = supply;
            }
            for (habitat, mut supply) in self.habitats.iter_mut().zip(supplies) {
                let current = habitat.utilities.supply_mut(kind);
                if supply.short() {
                    supply.shortage_minutes = current.shortage_minutes;
                }
                *current = supply;
            }
        }
    }

    /// One simulated minute: reallocate, count shortage time and the plants' service minutes.
    pub(crate) fn advance_utilities(&mut self) {
        self.refresh_utilities();
        let mut serving = Vec::new();
        for habitat in &mut self.habitats {
            for kind in UtilityKind::ALL {
                let supply = habitat.utilities.supply_mut(kind);
                if supply.short() {
                    supply.shortage_minutes =
                        (supply.shortage_minutes + 1).min(MAX_SHORTAGE_MINUTES);
                }
                serving.extend(supply.draws.iter().map(|draw| draw.concession_id));
            }
        }
        serving.sort_unstable();
        serving.dedup();
        for stand in &mut self.concessions {
            if serving.binary_search(&stand.id).is_ok() {
                stand.total_uses = stand.total_uses.saturating_add(1);
            }
        }
    }

    /// Units handed out by a plant, by habitat in service order.
    pub(crate) fn utility_consumers(&self, concession_id: u32) -> Vec<UtilityConsumerView> {
        let mut consumers: Vec<UtilityConsumerView> = self
            .habitats
            .iter()
            .flat_map(|habitat| {
                UtilityKind::ALL.into_iter().filter_map(move |kind| {
                    let supply = habitat.utilities.supply(kind);
                    let units = supply
                        .draws
                        .iter()
                        .filter(|draw| draw.concession_id == concession_id)
                        .map(|draw| draw.units)
                        .sum::<u32>();
                    (units > 0).then_some(UtilityConsumerView {
                        habitat_id: habitat.id,
                        units,
                        rank: supply.rank,
                        priority: habitat.utilities.priority.key(),
                    })
                })
            })
            .collect();
        consumers.sort_by_key(|consumer| (consumer.rank, consumer.habitat_id));
        consumers
    }

    /// Habitats left short because this working plant, which they reach, ran out of capacity.
    pub(crate) fn utility_overloaded_for(&self, concession_id: u32) -> Vec<u32> {
        self.habitats
            .iter()
            .filter(|habitat| {
                UtilityKind::ALL.into_iter().any(|kind| {
                    let supply = habitat.utilities.supply(kind);
                    supply.reason == Some(ShortageReason::Overloaded)
                        && supply.reachable.contains(&concession_id)
                })
            })
            .map(|habitat| habitat.id)
            .collect()
    }

    pub(crate) fn utility_facility_view(&self, index: usize) -> Option<UtilityFacilityView> {
        let stand = &self.concessions[index];
        let service = stand.definition.utility?;
        let consumers = self.utility_consumers(stand.id);
        let allocated = consumers.iter().map(|consumer| consumer.units).sum();
        let working = stand.condition > super::MAINTENANCE_FAILURE_THRESHOLD;
        let short_habitats = self.utility_overloaded_for(stand.id);
        let needs_service = stand.condition <= super::MAINTENANCE_SERVICE_THRESHOLD;
        let status = if !working {
            format!(
                "Failed: supplies nothing until a mechanic repairs it ({}) or it is replaced ({})",
                dollars(self.catalog.balance.maintenance_repair_cents),
                dollars(stand.definition.build_cost_cents)
            )
        } else if !short_habitats.is_empty() {
            format!(
                "Overloaded: {allocated}/{} units handed out; habitat {} still short",
                service.capacity,
                list(&short_habitats)
            )
        } else if consumers.is_empty() {
            format!(
                "Idle: no habitat within {} path tiles needs {}",
                service.coverage_tiles,
                service.kind.label().to_lowercase()
            )
        } else {
            format!("Running: {allocated}/{} units in use", service.capacity)
        };
        Some(UtilityFacilityView {
            kind: service.kind,
            capacity: service.capacity,
            coverage_tiles: service.coverage_tiles,
            allocated,
            working,
            overloaded: !short_habitats.is_empty(),
            short_habitats,
            consumers,
            hourly_operating_cents: if working {
                stand.definition.hourly_upkeep_cents
            } else {
                0
            },
            replacement_cents: stand.definition.build_cost_cents,
            can_replace: needs_service,
            service_minutes: stand.total_uses,
            status,
        })
    }

    pub(crate) fn habitat_utilities_view(&self, habitat: &Habitat) -> HabitatUtilitiesView {
        let supply = |kind: UtilityKind| {
            let supply = habitat.utilities.supply(kind);
            let status = if supply.demand == 0 {
                "not_needed"
            } else if !supply.short() {
                "supplied"
            } else if supply.degraded() {
                "degraded"
            } else {
                "short"
            };
            let detail = if supply.demand == 0 {
                "Not needed by these animals".to_owned()
            } else if !supply.short() {
                format!(
                    "{} units from {}",
                    supply.demand,
                    supply
                        .draws
                        .iter()
                        .map(|draw| format!("#{} ({})", draw.concession_id, draw.units))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                let left = SHORTAGE_GRACE_MINUTES.saturating_sub(supply.shortage_minutes);
                format!(
                    "{} of {} units: {}. {}",
                    supply.supplied,
                    supply.demand,
                    supply.reason.map_or("short", reason_text),
                    if left > 0 {
                        format!("Welfare is affected in {left} min unless it is fixed")
                    } else {
                        "The animals are uncomfortable until it is fixed".to_owned()
                    }
                )
            };
            SupplyView {
                kind,
                label: kind.label(),
                demand: supply.demand,
                supplied: supply.supplied,
                status,
                reason: supply.reason,
                detail,
                fix: supply
                    .reason
                    .map(|reason| self.shortage_fix(kind, reason, supply)),
                shortage_minutes: supply.shortage_minutes,
                rank: supply.rank,
                draws: supply.draws.clone(),
                reachable: supply.reachable.clone(),
            }
        };
        HabitatUtilitiesView {
            priority: habitat.utilities.priority.key(),
            water: supply(UtilityKind::Water),
            climate: supply(UtilityKind::Climate),
        }
    }

    /// What restores supply, said for the player.
    pub(crate) fn shortage_fix(
        &self,
        kind: UtilityKind,
        reason: ShortageReason,
        supply: &Supply,
    ) -> String {
        match reason {
            ShortageReason::NoneBuilt => format!("Build a {} beside a path", kind.plant()),
            ShortageReason::Unconnected => format!(
                "Connect a {} to the service gate with a path, or build one nearer",
                kind.plant()
            ),
            ShortageReason::Failed => {
                let failed: Vec<u32> = supply
                    .reachable
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.concessions.iter().any(|stand| {
                            stand.id == *id
                                && stand.condition <= super::MAINTENANCE_FAILURE_THRESHOLD
                        })
                    })
                    .collect();
                if self.mechanics.is_empty() {
                    format!(
                        "Hire a mechanic to repair {} {}, or replace it",
                        kind.plant(),
                        list(&failed)
                    )
                } else {
                    format!(
                        "A mechanic will repair {} {}; replace it to restore supply now",
                        kind.plant(),
                        list(&failed)
                    )
                }
            }
            ShortageReason::Overloaded => format!(
                "Build another {} or raise this habitat's utility priority",
                kind.plant()
            ),
        }
    }

    pub(crate) fn set_habitat_utility_priority(
        &mut self,
        habitat_id: u32,
        priority: &str,
    ) -> ActionResult {
        let Some(priority) = UtilityPriority::parse(priority) else {
            return ActionResult::error("Utility priority must be high, normal or low");
        };
        let Some(index) = self.find_habitat(habitat_id) else {
            return ActionResult::error(format!("Habitat #{habitat_id} does not exist"));
        };
        if self.habitats[index].utilities.priority == priority {
            return ActionResult::ok(format!(
                "Habitat #{habitat_id} already has {} utility priority",
                priority.key()
            ));
        }
        self.habitats[index].utilities.priority = priority;
        self.refresh_utilities();
        ActionResult::ok(format!(
            "Habitat #{habitat_id} now has {} utility priority",
            priority.key()
        ))
    }

    /// Buys a new plant in place of a worn or failed one: it runs at full condition at once and
    /// its open maintenance task, if any, is withdrawn.
    pub(crate) fn replace_utility(&mut self, concession_id: u32) -> ActionResult {
        let Some(index) = self
            .concessions
            .iter()
            .position(|stand| stand.id == concession_id)
        else {
            return ActionResult::error(format!("Facility #{concession_id} does not exist"));
        };
        let stand = &self.concessions[index];
        if stand.definition.utility.is_none() {
            return ActionResult::error(format!(
                "{} #{concession_id} is not a utility plant",
                stand.definition.label
            ));
        }
        let label = stand.definition.label.clone();
        if stand.condition > super::MAINTENANCE_SERVICE_THRESHOLD {
            return ActionResult::error(format!(
                "{label} #{concession_id} is in good order ({}% condition)",
                stand.condition
            ));
        }
        let cost = stand.definition.build_cost_cents;
        if let Err(message) = self.spend(cost, ExpenseCategory::UtilityReplacement) {
            return ActionResult::error(message);
        }
        self.concessions[index].condition = 100;
        let withdrawn: Vec<u32> = self
            .maintenance
            .iter()
            .filter(|task| task.concession_id == concession_id)
            .map(|task| task.id)
            .collect();
        self.maintenance
            .retain(|task| task.concession_id != concession_id);
        for mechanic in &mut self.mechanics {
            if mechanic
                .target_maintenance_id
                .is_some_and(|id| withdrawn.contains(&id))
            {
                mechanic.target_maintenance_id = None;
            }
        }
        self.refresh_utilities();
        ActionResult::ok(format!(
            "{label} #{concession_id} replaced for {}",
            dollars(cost)
        ))
    }

    /// A habitat short of a utility, with its reason and fix; a plant whose capacity ran out.
    pub(crate) fn detect_utility_alerts(
        &self,
        raise: &mut impl FnMut(AlertCode, Severity, Subject, Position, String, AlertAction),
    ) {
        for habitat in &self.habitats {
            for kind in UtilityKind::ALL {
                let supply = habitat.utilities.supply(kind);
                let Some(reason) = supply.reason else {
                    continue;
                };
                let failed = supply.reachable.iter().find_map(|id| {
                    self.concessions.iter().find(|stand| {
                        stand.id == *id && stand.condition <= super::MAINTENANCE_FAILURE_THRESHOLD
                    })
                });
                let (focus, action) = match reason {
                    ShortageReason::NoneBuilt | ShortageReason::Overloaded => (
                        GameState::habitat_center(habitat),
                        match kind {
                            UtilityKind::Water => AlertAction::BuildWaterPump,
                            UtilityKind::Climate => AlertAction::BuildClimatePlant,
                        },
                    ),
                    ShortageReason::Unconnected => (
                        self.gate_access_tile(habitat.service_gate)
                            .unwrap_or_else(|| GameState::habitat_center(habitat)),
                        AlertAction::BuildPath,
                    ),
                    ShortageReason::Failed => (
                        failed.map_or_else(
                            || GameState::habitat_center(habitat),
                            |stand| Position {
                                x: stand.x,
                                y: stand.y,
                            },
                        ),
                        if self.mechanics.is_empty() {
                            AlertAction::HireMechanic
                        } else {
                            AlertAction::InspectFacility
                        },
                    ),
                };
                let left = SHORTAGE_GRACE_MINUTES.saturating_sub(supply.shortage_minutes);
                let effect = if left > 0 {
                    format!("welfare is affected in {left} min")
                } else {
                    "the animals are uncomfortable".to_owned()
                };
                raise(
                    match kind {
                        UtilityKind::Water => AlertCode::WaterShortage,
                        UtilityKind::Climate => AlertCode::ClimateShortage,
                    },
                    Severity::Warning,
                    Subject::Habitat(habitat.id),
                    focus,
                    format!(
                        "Habitat #{} gets {} of {} {} units because {}; {effect}. {}.",
                        habitat.id,
                        supply.supplied,
                        supply.demand,
                        kind.label().to_lowercase(),
                        reason_text(reason),
                        self.shortage_fix(kind, reason, supply)
                    ),
                    action,
                );
            }
        }
        for stand in &self.concessions {
            let Some(service) = stand.definition.utility else {
                continue;
            };
            if stand.condition <= super::MAINTENANCE_FAILURE_THRESHOLD {
                continue;
            }
            let short = self.utility_overloaded_for(stand.id);
            if short.is_empty() {
                continue;
            }
            raise(
                AlertCode::UtilityOverloaded,
                Severity::Warning,
                Subject::Concession(stand.id),
                Position {
                    x: stand.x,
                    y: stand.y,
                },
                format!(
                    "{} #{} is overloaded: all {} units are in use and habitat {} is still short. Build another {} or change utility priorities.",
                    stand.definition.label,
                    stand.id,
                    service.capacity,
                    list(&short),
                    service.kind.plant()
                ),
                match service.kind {
                    UtilityKind::Water => AlertAction::BuildWaterPump,
                    UtilityKind::Climate => AlertAction::BuildClimatePlant,
                },
            );
        }
    }

    /// Stored allocations must name existing plants of their kind, add up, and never exceed
    /// a plant's capacity.
    pub(crate) fn utility_violation(&self) -> Option<String> {
        let mut handed: Vec<(u32, u32)> = Vec::new();
        for habitat in &self.habitats {
            for kind in UtilityKind::ALL {
                let supply = habitat.utilities.supply(kind);
                let drawn: u32 = supply.draws.iter().map(|draw| draw.units).sum();
                if drawn != supply.supplied || supply.supplied > supply.demand {
                    return Some(format!(
                        "habitat {} {} supply does not add up",
                        habitat.id,
                        kind.key()
                    ));
                }
                if supply.shortage_minutes > MAX_SHORTAGE_MINUTES {
                    return Some(format!(
                        "habitat {} {} shortage time is out of range",
                        habitat.id,
                        kind.key()
                    ));
                }
                for draw in &supply.draws {
                    let plant = self
                        .concessions
                        .iter()
                        .find(|stand| stand.id == draw.concession_id)
                        .and_then(|stand| stand.definition.utility);
                    if plant.is_none_or(|plant| plant.kind != kind) {
                        return Some(format!(
                            "habitat {} draws {} from a missing plant {}",
                            habitat.id,
                            kind.key(),
                            draw.concession_id
                        ));
                    }
                    match handed.iter_mut().find(|(id, _)| *id == draw.concession_id) {
                        Some((_, units)) => *units += draw.units,
                        None => handed.push((draw.concession_id, draw.units)),
                    }
                }
            }
        }
        for (id, units) in handed {
            let capacity = self
                .concessions
                .iter()
                .find(|stand| stand.id == id)
                .and_then(|stand| stand.definition.utility)
                .map_or(0, |plant| plant.capacity);
            if units > capacity {
                return Some(format!("plant {id} hands out more than its capacity"));
            }
        }
        None
    }
}

fn list(ids: &[u32]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Serialize)]
pub(crate) struct UtilityConsumerView {
    pub habitat_id: u32,
    pub units: u32,
    pub rank: u32,
    pub priority: &'static str,
}

/// A plant's inspector.
#[derive(Debug, Serialize)]
pub(crate) struct UtilityFacilityView {
    pub kind: UtilityKind,
    pub capacity: u32,
    pub coverage_tiles: u32,
    pub allocated: u32,
    pub working: bool,
    pub overloaded: bool,
    /// Habitats it reaches that are short because its capacity ran out.
    pub short_habitats: Vec<u32>,
    pub consumers: Vec<UtilityConsumerView>,
    pub hourly_operating_cents: i64,
    pub replacement_cents: i64,
    pub can_replace: bool,
    /// Simulated minutes it has supplied anything.
    pub service_minutes: u32,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SupplyView {
    pub kind: UtilityKind,
    pub label: &'static str,
    pub demand: u32,
    pub supplied: u32,
    /// `not_needed`, `supplied`, `short` (warning hour) or `degraded`.
    pub status: &'static str,
    pub reason: Option<ShortageReason>,
    pub detail: String,
    pub fix: Option<String>,
    pub shortage_minutes: u32,
    pub rank: u32,
    pub draws: Vec<Draw>,
    pub reachable: Vec<u32>,
}

/// A habitat inspector's utilities section.
#[derive(Debug, Serialize)]
pub(crate) struct HabitatUtilitiesView {
    pub priority: &'static str,
    pub water: SupplyView,
    pub climate: SupplyView,
}
