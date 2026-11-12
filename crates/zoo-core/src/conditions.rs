//! The park calendar, daylight, scenario weather and bounded park events (#118).
//!
//! Time is the simulation clock, never wall-clock time: the day number and minute of day decide
//! the season, the daylight the renderer presents and the weather, so pausing, speed changes,
//! a backgrounded browser or graphics settings can neither skip nor repeat anything.
//!
//! A scenario may define a weather stream ([`ConditionsDefinition`]). Each day's weather is a
//! pure function of the park's scenario seed, the day and the definition, so a forecast is
//! exact and identical across save/load. The one condition is a hot day: animals without shade
//! or working climate control are less comfortable, and guests get thirsty faster. Heat comes in
//! runs of at most two days, the comfort part never falls below [`HEAT_FLOOR`] and a habitat is
//! warned from the day before, so the same unresolved condition can only cost a bounded amount
//! and always leaves time to add shelter.
//!
//! The one authored event is a school visit, announced a day ahead: a class arrives at the gate
//! as one visitor group through the ordinary admission, education and donation rules. At most
//! one event is announced at a time, each is followed by a cooldown, and a missed visit (the gate
//! closed, nothing to see) costs nothing. Both are rules the player may switch off; presentation
//! settings (lighting, weather visuals) live in the browser and never change outcomes.
use super::{
    ActionResult, GameState, Habitat, Position,
    alerts::{AlertAction, AlertCode, Severity, Subject},
    animal,
    utility::UtilityKind,
    visit::{GroupArrival, Profile},
};
use crate::pricing::GateOutcome;
use serde::{Deserialize, Serialize};

pub(crate) const MAX_SEASON_DAYS: u32 = 28;
/// Seasons last this long in parks whose scenario defines no weather.
const DEFAULT_SEASON_DAYS: u32 = 7;
pub(crate) const MAX_SCHOOL_STUDENTS: u32 = 12;
const MAX_COOLDOWN_DAYS: u32 = 28;
/// Days the forecast covers, today included.
pub(crate) const FORECAST_DAYS: u32 = 3;
/// An event is announced this many days before it happens.
pub(crate) const EVENT_LEAD_DAYS: u32 = 1;
/// Event outcomes the park remembers.
const MAX_HISTORY: usize = 8;
/// A hot habitat with no shade at all still keeps this comfort score.
pub(crate) const HEAT_FLOOR: u32 = 60;
/// During heat a guest gains one extra thirst point every this many minutes.
pub(crate) const HEAT_THIRST_INTERVAL_MINUTES: u32 = 3;
/// Dawn and dusk each last this long around sunrise and sunset.
const TWILIGHT_MINUTES: u32 = 60;
const HEAT_SALT: u32 = 0x4845_4154;
const EVENT_SALT: u32 = 0x4556_4e54;
const MINUTES_PER_DAY: u32 = 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    const ALL: [Self; 4] = [Self::Spring, Self::Summer, Self::Autumn, Self::Winter];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Spring => "Spring",
            Self::Summer => "Summer",
            Self::Autumn => "Autumn",
            Self::Winter => "Winter",
        }
    }

    /// Sunrise and sunset, in minutes of the day.
    fn daylight(self) -> (u32, u32) {
        match self {
            Self::Spring => (6 * 60, 20 * 60),
            Self::Summer => (5 * 60, 21 * 60),
            Self::Autumn => (7 * 60, 19 * 60),
            Self::Winter => (8 * 60, 17 * 60),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Weather {
    #[default]
    Fair,
    /// A hot day: shade and climate control keep animals comfortable; guests drink more.
    Heat,
}

impl Weather {
    fn label(self) -> &'static str {
        match self {
            Self::Fair => "Fair",
            Self::Heat => "Hot",
        }
    }
}

/// A scenario's weather stream and events, copied into each park it creates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConditionsDefinition {
    /// Days per season, `1..=28`; the calendar starts on the first day of spring.
    pub season_days: u32,
    /// The chance of a hot day in each season (spring, summer, autumn, winter), `0..=100`.
    pub heat_percent: [u32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub school_visit: Option<SchoolVisitDefinition>,
}

/// A class that may visit on an announced day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SchoolVisitDefinition {
    /// The chance, `1..=100`, that an eligible day brings a visit.
    pub chance_percent: u32,
    /// Students in the class, `2..=12`.
    pub students: u32,
    /// When the class reaches the gate, a minute of the day between 06:00 and 18:00.
    pub arrival_minute: u32,
    /// Days after a visit before the next can happen, `1..=28`.
    pub cooldown_days: u32,
}

impl ConditionsDefinition {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_SEASON_DAYS).contains(&self.season_days) {
            return Err(format!("season_days: expected 1..={MAX_SEASON_DAYS}"));
        }
        if self.heat_percent.iter().any(|percent| *percent > 100) {
            return Err("heat_percent: each season's chance is 0..=100".to_owned());
        }
        if let Some(visit) = &self.school_visit {
            if !(1..=100).contains(&visit.chance_percent) {
                return Err("school_visit/chance_percent: expected 1..=100".to_owned());
            }
            if !(2..=MAX_SCHOOL_STUDENTS).contains(&visit.students) {
                return Err(format!(
                    "school_visit/students: expected 2..={MAX_SCHOOL_STUDENTS}"
                ));
            }
            if !(6 * 60..=18 * 60).contains(&visit.arrival_minute) {
                return Err("school_visit/arrival_minute: expected 360..=1080".to_owned());
            }
            if !(1..=MAX_COOLDOWN_DAYS).contains(&visit.cooldown_days) {
                return Err(format!(
                    "school_visit/cooldown_days: expected 1..={MAX_COOLDOWN_DAYS}"
                ));
            }
        }
        Ok(())
    }
}

/// The player's rules. Switching one off changes outcomes, unlike presentation settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rules {
    /// Whether weather affects animals and guests.
    pub weather_effects: bool,
    /// Whether events are announced and happen.
    pub events: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            weather_effects: true,
            events: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventKind {
    SchoolVisit,
}

impl EventKind {
    fn label(self) -> &'static str {
        match self {
            Self::SchoolVisit => "School visit",
        }
    }
}

/// The announced event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScheduledEvent {
    pub kind: EventKind,
    pub day: u32,
    pub minute: u32,
    pub guests: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventOutcome {
    /// The class was admitted and is touring the park.
    Arrived,
    /// The gate was closed when the class came.
    ParkClosed,
    /// The class found admission too expensive or over its budget.
    TurnedAway,
    /// Nothing stocked could be reached, so the class did not come.
    NothingToSee,
    /// The player switched events off before it happened.
    Cancelled,
}

impl EventOutcome {
    fn text(self) -> &'static str {
        match self {
            Self::Arrived => "arrived",
            Self::ParkClosed => "missed: the gate was closed",
            Self::TurnedAway => "missed: admission was too expensive",
            Self::NothingToSee => "missed: no stocked habitat could be reached",
            Self::Cancelled => "cancelled: events were switched off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventRecord {
    pub kind: EventKind,
    pub day: u32,
    pub outcome: EventOutcome,
    pub guests: u32,
    /// The admitted group, if the class arrived.
    pub group_id: Option<u32>,
}

/// A park's calendar state: its own copy of the scenario's stream, the rules and its events.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Conditions {
    /// `None` keeps every day fair and announces no events.
    pub definition: Option<ConditionsDefinition>,
    pub rules: Rules,
    /// At most one announced event.
    pub scheduled: Option<ScheduledEvent>,
    /// The last day already considered for an event; each day is considered exactly once.
    pub planned_through_day: u32,
    /// The first day a new event may happen (after the previous event's cooldown).
    pub next_event_day: u32,
    /// The latest event outcomes, oldest first.
    pub history: Vec<EventRecord>,
}

impl Conditions {
    pub(crate) fn new(definition: Option<ConditionsDefinition>) -> Self {
        Self {
            definition,
            rules: Rules::default(),
            scheduled: None,
            planned_through_day: 1,
            next_event_day: 1,
            history: Vec::new(),
        }
    }

    fn season_days(&self) -> u32 {
        self.definition
            .as_ref()
            .map_or(DEFAULT_SEASON_DAYS, |definition| definition.season_days)
    }

    pub(crate) fn season_of(&self, day: u32) -> Season {
        let index = (day.saturating_sub(1) / self.season_days()) % 4;
        Season::ALL[index as usize]
    }

    /// Whether `day` rolls a hot day before the run bound is applied.
    fn heat_roll(&self, seed: u32, day: u32) -> bool {
        let Some(definition) = &self.definition else {
            return false;
        };
        if day == 0 {
            return false;
        }
        let chance = definition.heat_percent[self.season_of(day) as usize];
        animal::mix(animal::mix(seed, HEAT_SALT) ^ day, HEAT_SALT) % 100 < chance
    }

    /// The weather of `day`, whatever the rules say. Within a stretch of consecutive hot rolls
    /// every third day is fair, so heat runs last at most two days and a long hot spell keeps
    /// giving the park a cooler day to recover.
    pub(crate) fn weather_on(&self, seed: u32, day: u32) -> Weather {
        if !self.heat_roll(seed, day) {
            return Weather::Fair;
        }
        let since_start = (1..day)
            .rev()
            .take_while(|earlier| self.heat_roll(seed, *earlier))
            .count();
        if since_start % 3 == 2 {
            Weather::Fair
        } else {
            Weather::Heat
        }
    }

    fn event_roll(&self, seed: u32, day: u32, chance: u32) -> bool {
        animal::mix(animal::mix(seed, EVENT_SALT) ^ day, EVENT_SALT) % 100 < chance
    }

    pub(crate) fn violation(&self, day: u32, minute_of_day: u32) -> Option<String> {
        if let Some(definition) = &self.definition
            && let Err(error) = definition.validate()
        {
            return Some(format!("conditions/definition/{error}"));
        }
        if self.history.len() > MAX_HISTORY {
            return Some(format!("conditions: more than {MAX_HISTORY} event records"));
        }
        if self.planned_through_day > day.saturating_add(EVENT_LEAD_DAYS) {
            return Some("conditions: events are planned beyond the announcement lead".to_owned());
        }
        if let Some(event) = &self.scheduled {
            let future = (event.day, event.minute) > (day, minute_of_day);
            let school = self
                .definition
                .as_ref()
                .and_then(|definition| definition.school_visit.as_ref());
            if !future || event.minute >= MINUTES_PER_DAY {
                return Some("conditions: the announced event is in the past".to_owned());
            }
            if school.is_none_or(|visit| visit.students != event.guests) || !self.rules.events {
                return Some(
                    "conditions: the announced event is not in the park's rules".to_owned(),
                );
            }
        }
        None
    }
}

/// Where the sun is, for presentation: the renderer maps this to lighting.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct LightView {
    pub phase: &'static str,
    /// 0 at night, rising through dawn to 100 at noon.
    pub daylight_percent: u32,
    /// How far the sun is across the sky, 0 at sunrise and 1000 at sunset.
    pub sun_arc_permille: u32,
    pub sunrise_minute: u32,
    pub sunset_minute: u32,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ForecastDay {
    pub day: u32,
    pub season: Season,
    pub season_label: &'static str,
    pub weather: Weather,
    pub label: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ScheduledEventView {
    pub kind: EventKind,
    pub label: &'static str,
    pub day: u32,
    pub minute: u32,
    pub guests: u32,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EventRecordView {
    pub kind: EventKind,
    pub label: &'static str,
    pub day: u32,
    pub outcome: EventOutcome,
    pub guests: u32,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ConditionsView {
    pub season: Season,
    pub season_label: &'static str,
    /// The day of the season, from 1, and the season's length.
    pub season_day: u32,
    pub season_days: u32,
    /// Whether the scenario defines weather at all.
    pub weather_stream: bool,
    pub weather: Weather,
    /// Whether today's weather affects the park (it does unless the rule is off).
    pub effects_active: bool,
    pub summary: String,
    pub forecast: Vec<ForecastDay>,
    pub light: LightView,
    pub rules: Rules,
    /// Whether the scenario defines any event that could be announced.
    pub events_defined: bool,
    pub scheduled: Option<ScheduledEventView>,
    pub history: Vec<EventRecordView>,
}

fn clock(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

impl Habitat {
    /// Animals the habitat keeps cool on a hot day: reachable shelter places, or every animal
    /// while climate control supplies all it needs.
    pub(crate) fn shade_for(&self) -> u32 {
        let animals = self.animal_count();
        let climate = self.utilities.supply(UtilityKind::Climate);
        if self.utility_demand(UtilityKind::Climate) > 0 && !climate.short() {
            return animals;
        }
        self.assessment().shelter_capacity.min(animals)
    }

    /// The heat comfort part on a hot day; `None` on a fair day or with nobody living here.
    pub(crate) fn heat_part(&self) -> Option<(u32, String)> {
        if self.weather != Weather::Heat || !self.stocked() {
            return None;
        }
        let animals = self.animal_count();
        let shade = self.shade_for();
        let score = HEAT_FLOOR + (100 - HEAT_FLOOR) * shade / animals.max(1);
        Some((
            score,
            format!("Hot day: shade or cooling for {shade} of {animals} animals"),
        ))
    }
}

impl GameState {
    fn seed(&self) -> u32 {
        self.scenario.seed
    }

    /// Today's weather as it affects the park: fair while weather effects are switched off.
    pub(crate) fn effective_weather(&self) -> Weather {
        if self.conditions.rules.weather_effects {
            self.conditions.weather_on(self.seed(), self.day)
        } else {
            Weather::Fair
        }
    }

    pub(crate) fn heat_today(&self) -> bool {
        self.effective_weather() == Weather::Heat
    }

    /// Brings derived calendar state up to date: every habitat's weather, and the event
    /// announcements due by now. Idempotent; runs after every command and simulated minute.
    pub(crate) fn sync_conditions(&mut self) {
        let weather = self.effective_weather();
        for habitat in &mut self.habitats {
            habitat.weather = weather;
        }
        self.plan_events();
    }

    /// Considers each day once, a day ahead, for an announced event.
    fn plan_events(&mut self) {
        let horizon = self.day.saturating_add(EVENT_LEAD_DAYS);
        while self.conditions.planned_through_day < horizon {
            self.conditions.planned_through_day += 1;
            let day = self.conditions.planned_through_day;
            let conditions = &self.conditions;
            let Some(visit) = conditions
                .definition
                .as_ref()
                .and_then(|definition| definition.school_visit.as_ref())
            else {
                continue;
            };
            let eligible = conditions.rules.events
                && conditions.scheduled.is_none()
                && day > self.day
                && day >= conditions.next_event_day
                && conditions.event_roll(self.seed(), day, visit.chance_percent);
            if eligible {
                self.conditions.scheduled = Some(ScheduledEvent {
                    kind: EventKind::SchoolVisit,
                    day,
                    minute: visit.arrival_minute,
                    guests: visit.students,
                });
                self.emit("event_announced", None);
            }
        }
    }

    /// Runs the announced event when its minute comes.
    pub(crate) fn advance_events(&mut self) {
        let Some(event) = self.conditions.scheduled else {
            return;
        };
        if (event.day, event.minute) != (self.day, self.minute_of_day) {
            return;
        }
        self.conditions.scheduled = None;
        let (outcome, group_id) = match self.arrive_group(Profile::School, event.guests) {
            GroupArrival::Admitted(group) => (EventOutcome::Arrived, Some(group)),
            GroupArrival::Refused(GateOutcome::Closed) => (EventOutcome::ParkClosed, None),
            GroupArrival::Refused(_) => (EventOutcome::TurnedAway, None),
            GroupArrival::NoAttraction | GroupArrival::NoRoom => (EventOutcome::NothingToSee, None),
        };
        let cooldown = self
            .conditions
            .definition
            .as_ref()
            .and_then(|definition| definition.school_visit.as_ref())
            .map_or(1, |visit| visit.cooldown_days);
        self.conditions.next_event_day = self.day.saturating_add(cooldown);
        self.record_event(EventRecord {
            kind: event.kind,
            day: event.day,
            outcome,
            guests: event.guests,
            group_id,
        });
        self.emit(
            if outcome == EventOutcome::Arrived {
                "school_visit_arrived"
            } else {
                "school_visit_missed"
            },
            group_id,
        );
    }

    fn record_event(&mut self, record: EventRecord) {
        let history = &mut self.conditions.history;
        history.push(record);
        if history.len() > MAX_HISTORY {
            history.remove(0);
        }
    }

    /// Switches a rule. Turning events off cancels the announced one; turning them back on
    /// announces again from the next day not yet considered.
    pub(crate) fn set_condition_rule(&mut self, rule: &str, enabled: bool) -> ActionResult {
        let (current, label) = match rule {
            "weather_effects" => (
                &mut self.conditions.rules.weather_effects,
                "Weather effects",
            ),
            "events" => (&mut self.conditions.rules.events, "Park events"),
            _ => return ActionResult::error(format!("Unknown park rule {rule}")),
        };
        let state = if enabled { "on" } else { "off" };
        if *current == enabled {
            return ActionResult::ok(format!("{label} are already {state}"));
        }
        *current = enabled;
        if rule == "events"
            && !enabled
            && let Some(event) = self.conditions.scheduled.take()
        {
            self.record_event(EventRecord {
                kind: event.kind,
                day: event.day,
                outcome: EventOutcome::Cancelled,
                guests: event.guests,
                group_id: None,
            });
        }
        self.sync_conditions();
        ActionResult::ok(format!("{label} switched {state}"))
    }

    pub(crate) fn light(&self) -> LightView {
        light_at(self.conditions.season_of(self.day), self.minute_of_day)
    }
}

/// The light at `minute` of a day in `season`.
fn light_at(season: Season, minute: u32) -> LightView {
    let (sunrise, sunset) = season.daylight();
    let arc = sunset - sunrise;
    let sun_arc_permille = minute.saturating_sub(sunrise).min(arc) * 1000 / arc;
    // A parabola over the day arc, 20 at sunrise and sunset, 100 at noon; twilight ramps
    // from 0 to 20 over the hour before sunrise and after sunset.
    let daylight_percent = if (sunrise..=sunset).contains(&minute) {
        let t = sun_arc_permille;
        20 + 80 * (4 * t * (1000 - t) / 1000) / 1000
    } else if minute + TWILIGHT_MINUTES >= sunrise && minute < sunrise {
        20 * (minute + TWILIGHT_MINUTES - sunrise) / TWILIGHT_MINUTES
    } else if minute > sunset && minute < sunset + TWILIGHT_MINUTES {
        20 * (sunset + TWILIGHT_MINUTES - minute) / TWILIGHT_MINUTES
    } else {
        0
    };
    let phase = if minute + TWILIGHT_MINUTES < sunrise || minute >= sunset + TWILIGHT_MINUTES {
        "night"
    } else if minute < sunrise + TWILIGHT_MINUTES {
        "dawn"
    } else if minute + TWILIGHT_MINUTES >= sunset {
        "dusk"
    } else {
        "day"
    };
    LightView {
        phase,
        daylight_percent,
        sun_arc_permille,
        sunrise_minute: sunrise,
        sunset_minute: sunset,
    }
}

impl GameState {
    /// Stocked habitats that will be hot today or tomorrow without shade for every animal.
    pub(crate) fn heat_warnings(&self) -> Vec<(u32, u32, u32, bool)> {
        if !self.conditions.rules.weather_effects {
            return Vec::new();
        }
        let seed = self.seed();
        let today = self.conditions.weather_on(seed, self.day) == Weather::Heat;
        let tomorrow =
            self.conditions.weather_on(seed, self.day.saturating_add(1)) == Weather::Heat;
        if !today && !tomorrow {
            return Vec::new();
        }
        self.habitats
            .iter()
            .filter(|habitat| habitat.stocked())
            .filter_map(|habitat| {
                let animals = habitat.animal_count();
                let shade = habitat.shade_for();
                (shade < animals).then_some((habitat.id, shade, animals, today))
            })
            .collect()
    }

    /// The heat warning of each habitat short of shade, from the day before a hot day: adding a
    /// shelter (or more shelter places) or working climate control resolves it.
    pub(crate) fn detect_heat_alerts(
        &self,
        raise: &mut impl FnMut(AlertCode, Severity, Subject, Position, String, AlertAction),
    ) {
        for (habitat_id, shade, animals, today) in self.heat_warnings() {
            let Some(habitat) = self
                .habitats
                .iter()
                .find(|habitat| habitat.id == habitat_id)
            else {
                continue;
            };
            let has_shelter = habitat.has_reachable_shelter();
            let fix = if has_shelter {
                "Place more shelter in the habitat or supply climate control"
            } else {
                "Add a shelter to the habitat"
            };
            raise(
                AlertCode::HeatShade,
                if today {
                    Severity::Warning
                } else {
                    Severity::Notice
                },
                Subject::Habitat(habitat_id),
                Self::habitat_center(habitat),
                format!(
                    "{} hot: habitat #{habitat_id} has shade or cooling for {shade} of its \
                     {animals} animals, so they will be less comfortable. {fix}.",
                    if today {
                        "Today is"
                    } else {
                        "Tomorrow will be"
                    },
                ),
                if has_shelter {
                    AlertAction::InspectHabitat
                } else {
                    AlertAction::AddShelter
                },
            );
        }
    }

    pub(crate) fn conditions_view(&self) -> ConditionsView {
        let conditions = &self.conditions;
        let seed = self.seed();
        let season = conditions.season_of(self.day);
        let season_days = conditions.season_days();
        let weather = conditions.weather_on(seed, self.day);
        let effects_active = conditions.rules.weather_effects;
        let summary = match (weather, effects_active) {
            (Weather::Fair, _) => "Fair weather".to_owned(),
            (Weather::Heat, true) => {
                "Hot day: animals need shade or climate control; guests drink more".to_owned()
            }
            (Weather::Heat, false) => "Hot day (weather effects are off)".to_owned(),
        };
        let forecast = (0..FORECAST_DAYS)
            .map(|offset| {
                let day = self.day.saturating_add(offset);
                let weather = conditions.weather_on(seed, day);
                let season = conditions.season_of(day);
                ForecastDay {
                    day,
                    season,
                    season_label: season.label(),
                    weather,
                    label: weather.label(),
                }
            })
            .collect();
        let scheduled = conditions.scheduled.map(|event| {
            let when = if event.day == self.day {
                "Today".to_owned()
            } else if event.day == self.day + 1 {
                "Tomorrow".to_owned()
            } else {
                format!("Day {}", event.day)
            };
            ScheduledEventView {
                kind: event.kind,
                label: event.kind.label(),
                day: event.day,
                minute: event.minute,
                guests: event.guests,
                message: format!(
                    "{when} at {}: a class of {} students arrives. They pay admission and learn \
                     from education signs and keeper talks; keep the gate open and a habitat \
                     reachable.",
                    clock(event.minute),
                    event.guests
                ),
            }
        });
        let history = conditions
            .history
            .iter()
            .rev()
            .map(|record| EventRecordView {
                kind: record.kind,
                label: record.kind.label(),
                day: record.day,
                outcome: record.outcome,
                guests: record.guests,
                message: format!(
                    "Day {}: {} of {} students {}",
                    record.day,
                    record.kind.label(),
                    record.guests,
                    record.outcome.text()
                ),
            })
            .collect();
        ConditionsView {
            season,
            season_label: season.label(),
            season_day: (self.day.saturating_sub(1) % season_days) + 1,
            season_days,
            weather_stream: conditions.definition.is_some(),
            weather,
            effects_active,
            summary,
            forecast,
            light: self.light(),
            rules: conditions.rules,
            events_defined: conditions
                .definition
                .as_ref()
                .is_some_and(|definition| definition.school_visit.is_some()),
            scheduled,
            history,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(heat_percent: [u32; 4]) -> Conditions {
        Conditions::new(Some(ConditionsDefinition {
            season_days: 4,
            heat_percent,
            school_visit: None,
        }))
    }

    #[test]
    fn heat_comes_in_runs_of_at_most_two_days_for_every_seed_and_chance() {
        for chance in [10, 60, 100] {
            let conditions = stream([chance; 4]);
            for seed in 0..64 {
                let mut run = 0;
                for day in 1..2_000 {
                    if conditions.weather_on(seed, day) == Weather::Heat {
                        run += 1;
                        assert!(
                            run <= 2,
                            "seed {seed} chance {chance}: day {day} is hot again"
                        );
                    } else {
                        run = 0;
                    }
                }
            }
        }
        // Certain heat alternates two hot days with a fair one.
        let always = stream([100; 4]);
        let days: Vec<_> = (1..=6).map(|day| always.weather_on(7, day)).collect();
        use Weather::{Fair, Heat};
        assert_eq!(days, [Heat, Heat, Fair, Heat, Heat, Fair]);
    }

    #[test]
    fn the_weather_follows_its_season_and_a_park_without_a_stream_stays_fair() {
        let summer_only = stream([0, 100, 0, 0]);
        for day in 1..200 {
            let summer = summer_only.season_of(day) == Season::Summer;
            if !summer {
                assert_eq!(summer_only.weather_on(1, day), Weather::Fair);
            }
        }
        assert_eq!(summer_only.season_of(1), Season::Spring);
        assert_eq!(summer_only.season_of(5), Season::Summer);
        assert_eq!(summer_only.season_of(13), Season::Winter);
        assert_eq!(summer_only.season_of(17), Season::Spring);
        let none = Conditions::new(None);
        assert!((1..500).all(|day| none.weather_on(5, day) == Weather::Fair));
        assert_eq!(none.season_of(8), Season::Summer);
    }

    #[test]
    fn daylight_rises_through_dawn_to_noon_and_falls_to_night() {
        for season in Season::ALL {
            let (sunrise, sunset) = season.daylight();
            assert_eq!(light_at(season, 0).phase, "night");
            assert_eq!(light_at(season, 0).daylight_percent, 0);
            assert_eq!(light_at(season, sunrise).phase, "dawn");
            assert_eq!(light_at(season, sunrise).daylight_percent, 20);
            let noon = light_at(season, sunrise.midpoint(sunset));
            assert_eq!((noon.phase, noon.daylight_percent), ("day", 100));
            assert_eq!(noon.sun_arc_permille, 500);
            assert_eq!(light_at(season, sunset).phase, "dusk");
            assert_eq!(light_at(season, sunset + TWILIGHT_MINUTES).phase, "night");
            // Rising every minute from night to noon, never above 100.
            let mut previous = 0;
            for minute in 0..=sunrise.midpoint(sunset) {
                let light = light_at(season, minute);
                assert!(light.daylight_percent >= previous && light.daylight_percent <= 100);
                previous = light.daylight_percent;
            }
        }
        let (summer, winter) = (Season::Summer.daylight(), Season::Winter.daylight());
        assert!(
            summer.1 - summer.0 > winter.1 - winter.0,
            "summer days are longer"
        );
    }

    #[test]
    fn definitions_outside_their_bounds_are_refused() {
        let valid = ConditionsDefinition {
            season_days: 4,
            heat_percent: [0, 60, 20, 0],
            school_visit: Some(SchoolVisitDefinition {
                chance_percent: 35,
                students: 8,
                arrival_minute: 600,
                cooldown_days: 4,
            }),
        };
        assert_eq!(valid.validate(), Ok(()));
        let broken = |change: fn(&mut ConditionsDefinition)| {
            let mut definition = valid.clone();
            change(&mut definition);
            definition.validate().unwrap_err()
        };
        assert!(broken(|d| d.season_days = 0).starts_with("season_days"));
        assert!(broken(|d| d.heat_percent[1] = 101).starts_with("heat_percent"));
        fn visit(d: &mut ConditionsDefinition) -> &mut SchoolVisitDefinition {
            d.school_visit.as_mut().unwrap()
        }
        assert!(broken(|d| visit(d).students = 13).contains("students"));
        assert!(broken(|d| visit(d).chance_percent = 0).contains("chance_percent"));
        assert!(broken(|d| visit(d).arrival_minute = 23 * 60).contains("arrival_minute"));
        assert!(broken(|d| visit(d).cooldown_days = 0).contains("cooldown_days"));
    }
}
