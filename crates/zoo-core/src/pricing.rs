//! Player-controlled prices, the park gate and explainable demand (#106).
//!
//! The player sets one admission price per person, a price per food or drink stand, and whether
//! the park is open. Every value decision stays here in `zoo-core`; the browser only edits the
//! numbers and shows the report.
//!
//! **Spending state.** Each person in an arriving group carries one seeded, finite budget,
//! drawn from the group's place in the arrival sequence. Admission is paid out of that budget
//! at the gate and whatever is left is the guest's spending money for stands (#102), so the
//! same money pays for admission and concessions.
//!
//! **The gate.** A group that can reach an attraction (#103) comes to the gate and decides
//! there, once, before anyone enters. The park's *experience value* is what one person thinks
//! the reachable park is worth: a base amount plus an amount per stocked, engaging habitat a
//! guest can walk to from the entrance (more for higher welfare, at most
//! [`VALUED_HABITATS`] habitats), so a habitat nobody can reach adds nothing and can never
//! stop a group from coming to the habitats it can reach. Each group has a seeded price
//! tolerance on top of its profile's (families are the most price-sensitive, enthusiasts the
//! least), and pays at most `experience value × tolerance` per person. A closed park, a price
//! above that, or a price above any member's budget turns the whole group away: it pays
//! nothing and nobody enters. Otherwise every member pays the price once, in one ledger line
//! for the group. Turned-away groups still take their place in the arrival sequence, so who
//! comes next never depends on earlier decisions.
//!
//! **Value response.** A price at or below the experience value is fair; guests arrive feeling
//! better about a bargain, and a premium lowers their value perception in proportion, so dear
//! tickets bring fewer, less satisfied guests who leave earlier for poor value (#103) and buy
//! less. At a stand, each guest will pay up to the stand's authored reference price times its
//! own seeded tolerance; above that the guest refuses, complains (a lost sale "too expensive",
//! less happiness and value) and the need stays unmet. A guest agrees the stand's price when
//! a vendor starts serving it, and pays exactly that price when the sale completes; a later
//! price change never alters it.
//!
//! **Forecast.** The admission forecast replays the last [`RECENT_GATE`] groups at the gate
//! against a candidate price. It states its assumptions: the same groups and the same park;
//! it is evidence, not a promise.
//!
//! **Marketing.** A campaign costs a fixed amount once, runs for one day and, while it runs,
//! brings groups to the gate more often (a bounded, fixed rate). Only one runs at a time.
use super::{
    ActionResult, ExpenseCategory, GameState, Guest, MIN_ENGAGING_HABITAT_WELFARE, Mover,
    ViewingOccupancy, animal,
    construction::dollars,
    visit::{ARRIVAL_MINUTES_PER_GUEST, DistanceField, Profile},
};
use serde::{Deserialize, Serialize};

/// The highest admission price per person.
pub(crate) const MAX_ADMISSION_CENTS: i64 = 10_000;
/// The highest price at a stand.
pub(crate) const MAX_STAND_PRICE_CENTS: i64 = 5_000;
/// What any reachable park is worth to one person before its habitats.
const BASE_EXPERIENCE_CENTS: i64 = 600;
/// What each reachable, engaging habitat adds.
const HABITAT_EXPERIENCE_CENTS: i64 = 700;
/// What each welfare point of such a habitat adds on top.
const WELFARE_EXPERIENCE_CENTS: i64 = 5;
/// Habitats beyond this many add nothing more.
pub(crate) const VALUED_HABITATS: usize = 6;
/// A seeded 0..=40 percent on top of the profile's price tolerance.
const TOLERANCE_SPREAD: u32 = 41;
/// Each person's budget: this base plus a seeded step of `0..=4` times [`BUDGET_STEP_CENTS`].
pub(crate) const BUDGET_BASE_CENTS: i64 = 2_000;
const BUDGET_STEP_CENTS: i64 = 400;
/// The value perception of a guest who paid exactly the experience value.
const FAIR_ARRIVAL_VALUE: i64 = 68;
/// The most a bargain (a free park) adds.
const BARGAIN_VALUE: i64 = 10;
/// What a premium of the full experience value again takes away.
const PREMIUM_VALUE: i64 = 30;
const MIN_ARRIVAL_VALUE: i64 = 30;
/// What a guest's price complaint at a stand costs its value perception.
pub(crate) const STAND_COMPLAINT_VALUE: u32 = 2;
/// A campaign's price, length and arrival pace (minutes per arriving person, instead of
/// [`ARRIVAL_MINUTES_PER_GUEST`]).
pub(crate) const CAMPAIGN_COST_CENTS: i64 = 25_000;
pub(crate) const CAMPAIGN_MINUTES: u64 = 24 * 60;
pub(crate) const CAMPAIGN_MINUTES_PER_GUEST: u32 = 16;
/// Groups the forecast remembers.
pub(crate) const RECENT_GATE: usize = 24;

const TOLERANCE_SALT: u32 = 0x5052_4943;
const GUEST_TOLERANCE_SALT: u32 = 0x5354_414e;
const BUDGET_SALT: u32 = 0x4255_4447;

/// What happened to a group at the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GateOutcome {
    Admitted,
    /// The park was closed.
    Closed,
    /// The price was above what the group thought the park worth.
    TooExpensive,
    /// The price was above a member's budget.
    OverBudget,
}

/// Arrivals at the gate over a period: potential arrivals, admissions and rejections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateCounts {
    /// Groups that came to the gate, admitted or not.
    pub groups_at_gate: u32,
    /// People in those groups: the potential arrivals.
    pub people_at_gate: u32,
    pub groups_admitted: u32,
    pub people_admitted: u32,
    /// People turned away, by reason.
    pub turned_away_closed: u32,
    pub turned_away_too_expensive: u32,
    pub turned_away_over_budget: u32,
    /// Admission charged.
    pub admission_cents: i64,
}

impl GateCounts {
    fn record(&mut self, outcome: GateOutcome, size: u32, charged: i64) {
        self.groups_at_gate = self.groups_at_gate.saturating_add(1);
        self.people_at_gate = self.people_at_gate.saturating_add(size);
        let slot = match outcome {
            GateOutcome::Admitted => {
                self.groups_admitted = self.groups_admitted.saturating_add(1);
                self.admission_cents = self.admission_cents.saturating_add(charged);
                &mut self.people_admitted
            }
            GateOutcome::Closed => &mut self.turned_away_closed,
            GateOutcome::TooExpensive => &mut self.turned_away_too_expensive,
            GateOutcome::OverBudget => &mut self.turned_away_over_budget,
        };
        *slot = slot.saturating_add(size);
    }

    fn turned_away(&self) -> u32 {
        self.turned_away_closed
            .saturating_add(self.turned_away_too_expensive)
            .saturating_add(self.turned_away_over_budget)
    }
}

/// One group's decision at the gate, kept for the report and the forecast.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateNote {
    /// The group's place in the arrival sequence.
    pub arrival: u32,
    /// The park minute (since day 1, 00:00) the group came.
    pub minute: u64,
    pub profile: Profile,
    pub size: u32,
    pub price_cents: i64,
    /// What one person thought the reachable park worth.
    pub value_cents: i64,
    /// The most each member would have paid: the group's limit or its smallest budget.
    pub willing_cents: i64,
    pub outcome: GateOutcome,
}

/// A running marketing campaign.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Campaign {
    pub started_minute: u64,
    pub ends_minute: u64,
}

/// The park's prices, gate and campaign (#106).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pricing {
    /// Admission per person, `0..=MAX_ADMISSION_CENTS`.
    pub admission_cents: i64,
    /// Whether the gate admits groups.
    pub open: bool,
    pub today: GateCounts,
    pub total: GateCounts,
    /// The latest groups at the gate, newest last.
    pub recent: Vec<GateNote>,
    pub campaign: Option<Campaign>,
    pub campaigns_run: u32,
}

impl Pricing {
    pub(crate) fn new(admission_cents: i64) -> Self {
        Self {
            admission_cents,
            open: true,
            today: GateCounts::default(),
            total: GateCounts::default(),
            recent: Vec::new(),
            campaign: None,
            campaigns_run: 0,
        }
    }
}

/// A decision at the gate, before anything is charged.
pub(crate) struct GateDecision {
    pub outcome: GateOutcome,
    pub value_cents: i64,
    pub willing_cents: i64,
    /// Each member's budget, in member order.
    pub budgets: Vec<i64>,
}

/// A price from the browser: whole cents in `0..=max`. Anything else (fractions, NaN,
/// infinities, negatives or out-of-range values) is refused with an explanation.
pub(crate) fn validate_price(cents: f64, max: i64, what: &str) -> Result<i64, String> {
    if !cents.is_finite() || cents.fract() != 0.0 {
        return Err(format!("{what} must be a whole number of cents"));
    }
    if cents < 0.0 || cents > max as f64 {
        return Err(format!("{what} must be between $0 and {}", dollars(max)));
    }
    Ok(cents as i64)
}

/// A group's price tolerance in percent of the experience value.
pub(crate) fn group_tolerance(arrival: u32, profile: Profile) -> u32 {
    profile.rules().price_tolerance + animal::mix(arrival, TOLERANCE_SALT) % TOLERANCE_SPREAD
}

/// A guest's price tolerance at stands, in percent of the reference price.
pub(crate) fn guest_tolerance(guest_id: u32, profile: Profile) -> u32 {
    profile.rules().price_tolerance + animal::mix(guest_id, GUEST_TOLERANCE_SALT) % TOLERANCE_SPREAD
}

/// The budget of member `member` of the `arrival`-th group.
pub(crate) fn member_budget(arrival: u32, member: u32) -> i64 {
    let seed = animal::mix(animal::mix(arrival, BUDGET_SALT) ^ member, BUDGET_SALT);
    BUDGET_BASE_CENTS + i64::from(seed % 5) * BUDGET_STEP_CENTS
}

/// `cents × percent / 100`, rounded down.
pub(crate) fn percent_of(cents: i64, percent: u32) -> i64 {
    cents.saturating_mul(i64::from(percent)) / 100
}

/// How a guest who paid `price` for a park worth `value` feels about its value on arrival.
pub(crate) fn arrival_value(price: i64, value: i64) -> u32 {
    let value = value.max(1);
    let feeling = if price <= value {
        FAIR_ARRIVAL_VALUE + BARGAIN_VALUE * (value - price) / value
    } else {
        FAIR_ARRIVAL_VALUE - PREMIUM_VALUE.saturating_mul(price - value) / value
    };
    feeling.clamp(MIN_ARRIVAL_VALUE, 100) as u32
}

/// The admission forecast at one price, with its assumptions spelled out.
#[derive(Serialize)]
pub(crate) struct AdmissionForecast {
    pub price_cents: i64,
    /// Groups and people the forecast is based on.
    pub sample_groups: u32,
    pub sample_people: u32,
    /// Of those, the ones who would pay this price.
    pub accepting_groups: u32,
    pub accepting_people: u32,
    /// Admission those people would pay.
    pub admission_cents: i64,
    pub assumptions: String,
}

#[derive(Serialize)]
pub(crate) struct CampaignView {
    pub ends_in_minutes: u64,
}

/// One stand's price and demand for the pricing panel.
#[derive(Serialize)]
pub(crate) struct StandPriceView {
    pub id: u32,
    pub kind: String,
    pub label: String,
    pub price_cents: i64,
    pub reference_price_cents: i64,
    pub served_today: u32,
    pub revenue_today_cents: i64,
    pub too_expensive_today: u32,
    pub unaffordable_today: u32,
    pub lost_today: u32,
    pub contribution_today_cents: i64,
    pub contribution_total_cents: i64,
}

/// The pricing panel's whole report (#106).
#[derive(Serialize)]
pub(crate) struct PricingView {
    pub open: bool,
    pub admission_price_cents: i64,
    pub max_admission_cents: i64,
    pub max_stand_price_cents: i64,
    /// What one person thinks the reachable park is worth now, and how many habitats count.
    pub experience_value_cents: i64,
    pub valued_habitats: u32,
    pub today: GateCounts,
    pub total: GateCounts,
    pub turned_away_today: u32,
    /// Refusals because a price was above what guests thought fair, today: at the gate
    /// (people) and at stands (sales).
    pub price_complaints_today: u32,
    /// People who left for poor value today (#103).
    pub poor_value_exits_today: u32,
    /// Mean value perception of the guests in the park; `None` when it is empty.
    pub average_value: Option<u32>,
    pub income_today_cents: i64,
    pub profit_today_cents: i64,
    pub forecast: AdmissionForecast,
    /// The latest groups at the gate, newest first.
    pub recent: Vec<GateNote>,
    pub stands: Vec<StandPriceView>,
    pub campaign: Option<CampaignView>,
    pub campaign_cost_cents: i64,
    pub campaign_minutes: u64,
    /// How much more often groups come while a campaign runs, in percent.
    pub campaign_arrival_boost_percent: u32,
}

impl GameState {
    /// What one person thinks the park reachable through `field` is worth, and how many
    /// habitats count towards it.
    pub(crate) fn experience_value(
        &self,
        field: &DistanceField,
        occupancy: &ViewingOccupancy,
    ) -> (i64, u32) {
        let mut habitats: Vec<i64> = self
            .habitats
            .iter()
            .filter(|habitat| {
                habitat.stocked() && habitat.welfare() >= MIN_ENGAGING_HABITAT_WELFARE
            })
            .filter(|habitat| self.best_viewpoint(habitat.id, field, occupancy).is_some())
            .map(|habitat| {
                HABITAT_EXPERIENCE_CENTS + WELFARE_EXPERIENCE_CENTS * i64::from(habitat.welfare())
            })
            .collect();
        habitats.sort_unstable_by(|left, right| right.cmp(left));
        habitats.truncate(VALUED_HABITATS);
        (
            BASE_EXPERIENCE_CENTS + habitats.iter().sum::<i64>(),
            habitats.len() as u32,
        )
    }

    /// The gate's decision for the `arrival`-th group of `size` people, at the current price.
    pub(crate) fn gate_decision(
        &self,
        arrival: u32,
        profile: Profile,
        size: u32,
        value_cents: i64,
    ) -> GateDecision {
        let price = self.pricing.admission_cents;
        let budgets: Vec<i64> = (0..size)
            .map(|member| member_budget(arrival, member))
            .collect();
        let limit = percent_of(value_cents, group_tolerance(arrival, profile));
        let smallest_budget = budgets.iter().copied().min().unwrap_or(0);
        let outcome = if !self.pricing.open {
            GateOutcome::Closed
        } else if price > limit {
            GateOutcome::TooExpensive
        } else if price > smallest_budget {
            GateOutcome::OverBudget
        } else {
            GateOutcome::Admitted
        };
        GateDecision {
            outcome,
            value_cents,
            willing_cents: limit.min(smallest_budget),
            budgets,
        }
    }

    /// Counts a group's decision at the gate and remembers it for the forecast.
    pub(crate) fn record_gate(
        &mut self,
        arrival: u32,
        profile: Profile,
        size: u32,
        decision: &GateDecision,
        charged: i64,
    ) {
        let minute = self.absolute_minute();
        let pricing = &mut self.pricing;
        pricing.today.record(decision.outcome, size, charged);
        pricing.total.record(decision.outcome, size, charged);
        pricing.recent.push(GateNote {
            arrival,
            minute,
            profile,
            size,
            price_cents: pricing.admission_cents,
            value_cents: decision.value_cents,
            willing_cents: decision.willing_cents,
            outcome: decision.outcome,
        });
        if pricing.recent.len() > RECENT_GATE {
            pricing.recent.remove(0);
        }
    }

    /// Simulated minutes per arriving person: faster while a campaign runs.
    pub(crate) fn arrival_minutes_per_guest(&self) -> u32 {
        if self.campaign_running() {
            CAMPAIGN_MINUTES_PER_GUEST
        } else {
            ARRIVAL_MINUTES_PER_GUEST
        }
    }

    fn campaign_running(&self) -> bool {
        let now = self.absolute_minute();
        self.pricing
            .campaign
            .is_some_and(|campaign| campaign.ends_minute > now)
    }

    /// Ends a campaign whose day is over.
    pub(crate) fn advance_campaign(&mut self) {
        if self.pricing.campaign.is_some() && !self.campaign_running() {
            self.pricing.campaign = None;
            self.emit("marketing_campaign_ended", None);
        }
    }

    /// The price tolerance of a guest at stands, from its group's profile.
    pub(crate) fn stand_tolerance(&self, guest: &Guest) -> u32 {
        let profile = self
            .group_index(guest.group_id)
            .map_or(Profile::Stroller, |index| self.groups[index].profile);
        guest_tolerance(guest.id, profile)
    }

    pub(crate) fn set_admission_price(&mut self, cents: f64) -> ActionResult {
        let cents = match validate_price(cents, MAX_ADMISSION_CENTS, "Admission") {
            Ok(cents) => cents,
            Err(message) => return ActionResult::error(message),
        };
        if self.pricing.admission_cents == cents {
            return ActionResult::ok(format!("Admission is already {}", dollars(cents)));
        }
        let before = self.pricing.admission_cents;
        self.pricing.admission_cents = cents;
        ActionResult::ok(format!(
            "Admission {} → {} per person; groups already inside keep what they paid",
            dollars(before),
            dollars(cents)
        ))
    }

    pub(crate) fn set_park_open(&mut self, open: bool) -> ActionResult {
        if self.pricing.open == open {
            return ActionResult::ok(format!(
                "The park is already {}",
                if open { "open" } else { "closed" }
            ));
        }
        self.pricing.open = open;
        ActionResult::ok(if open {
            "Park opened: groups are admitted again"
        } else {
            "Park closed: arriving groups are turned away; guests inside finish their visit"
        })
    }

    pub(crate) fn start_marketing_campaign(&mut self) -> ActionResult {
        if self.campaign_running() {
            return ActionResult::error("A marketing campaign is already running");
        }
        if let Err(message) = self.spend(CAMPAIGN_COST_CENTS, ExpenseCategory::Marketing) {
            return ActionResult::error(message);
        }
        let now = self.absolute_minute();
        self.pricing.campaign = Some(Campaign {
            started_minute: now,
            ends_minute: now + CAMPAIGN_MINUTES,
        });
        self.pricing.campaigns_run = self.pricing.campaigns_run.saturating_add(1);
        self.emit("marketing_campaign_started", None);
        ActionResult::ok(format!(
            "Marketing campaign started for {}: groups come {}% more often for a day",
            dollars(CAMPAIGN_COST_CENTS),
            campaign_boost_percent()
        ))
    }

    /// The forecast for `cents` from the latest groups at the gate.
    pub(crate) fn admission_forecast(&self, cents: i64) -> AdmissionForecast {
        let recent = &self.pricing.recent;
        let accepting = recent.iter().filter(|note| note.willing_cents >= cents);
        let accepting_groups = accepting.clone().count() as u32;
        let accepting_people: u32 = accepting.map(|note| note.size).sum();
        let assumptions = if recent.is_empty() {
            "No groups have come to the gate yet, so there is nothing to forecast from.".to_owned()
        } else {
            format!(
                "Replays the last {} groups at the gate against this price. Assumes the same visitors and the same park; real groups vary, and a dearer ticket also lowers how much guests value their visit.",
                recent.len()
            )
        };
        AdmissionForecast {
            price_cents: cents,
            sample_groups: recent.len() as u32,
            sample_people: recent.iter().map(|note| note.size).sum(),
            accepting_groups,
            accepting_people,
            admission_cents: cents.saturating_mul(i64::from(accepting_people)),
            assumptions,
        }
    }

    /// The forecast for a price the player is typing, validated like the command.
    pub(crate) fn evaluate_admission_price(&self, cents: f64) -> Result<AdmissionForecast, String> {
        validate_price(cents, MAX_ADMISSION_CENTS, "Admission")
            .map(|cents| self.admission_forecast(cents))
    }

    pub(crate) fn pricing_view(&self) -> PricingView {
        let occupancy = self.viewing_occupancy();
        let field = self.walk_distance_field(Mover::Guest, self.entrance);
        let (value, habitats) = self.experience_value(&field, &occupancy);
        let stands: Vec<StandPriceView> = self
            .concessions
            .iter()
            .filter_map(|stand| {
                let ops = stand.stand.as_ref()?;
                Some(StandPriceView {
                    id: stand.id,
                    kind: stand.definition.key.clone(),
                    label: format!("{} #{}", stand.definition.label, stand.id),
                    price_cents: ops.price_cents,
                    reference_price_cents: stand.definition.price_cents,
                    served_today: ops.today.served,
                    revenue_today_cents: ops.today.revenue_cents,
                    too_expensive_today: ops.today.lost.too_expensive,
                    unaffordable_today: ops.today.lost.unaffordable,
                    lost_today: ops.today.lost.total(),
                    contribution_today_cents: ops.today.contribution_cents(),
                    contribution_total_cents: ops.total.contribution_cents(),
                })
            })
            .collect();
        let stand_complaints: u32 = stands
            .iter()
            .map(|stand| stand.too_expensive_today)
            .fold(0, u32::saturating_add);
        let average_value = (!self.guests.is_empty()).then(|| {
            (self
                .guests
                .iter()
                .map(|guest| u64::from(guest.value_perception))
                .sum::<u64>()
                / self.guests.len() as u64) as u32
        });
        let now = self.absolute_minute();
        PricingView {
            open: self.pricing.open,
            admission_price_cents: self.pricing.admission_cents,
            max_admission_cents: MAX_ADMISSION_CENTS,
            max_stand_price_cents: MAX_STAND_PRICE_CENTS,
            experience_value_cents: value,
            valued_habitats: habitats,
            today: self.pricing.today,
            total: self.pricing.total,
            turned_away_today: self.pricing.today.turned_away(),
            price_complaints_today: self
                .pricing
                .today
                .turned_away_too_expensive
                .saturating_add(stand_complaints),
            poor_value_exits_today: self.visits.today.poor_value,
            average_value,
            income_today_cents: self.finance_today.income_total_cents(),
            profit_today_cents: self.finance_today.profit_cents(),
            forecast: self.admission_forecast(self.pricing.admission_cents),
            recent: self.pricing.recent.iter().rev().take(8).cloned().collect(),
            stands,
            campaign: self
                .pricing
                .campaign
                .filter(|campaign| campaign.ends_minute > now)
                .map(|campaign| CampaignView {
                    ends_in_minutes: campaign.ends_minute - now,
                }),
            campaign_cost_cents: CAMPAIGN_COST_CENTS,
            campaign_minutes: CAMPAIGN_MINUTES,
            campaign_arrival_boost_percent: campaign_boost_percent(),
        }
    }

    /// Pricing consistency, checked on load and with the per-tick invariants.
    pub(crate) fn pricing_violation(&self) -> Option<String> {
        let pricing = &self.pricing;
        if !(0..=MAX_ADMISSION_CENTS).contains(&pricing.admission_cents) {
            return Some(format!(
                "admission {} is outside 0..={MAX_ADMISSION_CENTS}",
                pricing.admission_cents
            ));
        }
        if pricing.recent.len() > RECENT_GATE {
            return Some("the gate remembers too many groups".to_owned());
        }
        for counts in [&pricing.today, &pricing.total] {
            let decided = counts
                .people_admitted
                .checked_add(counts.turned_away_closed)
                .and_then(|sum| sum.checked_add(counts.turned_away_too_expensive))
                .and_then(|sum| sum.checked_add(counts.turned_away_over_budget));
            if decided != Some(counts.people_at_gate)
                || counts.groups_admitted > counts.groups_at_gate
                || counts.admission_cents < 0
            {
                return Some("gate counts do not add up".to_owned());
            }
        }
        for stand in &self.concessions {
            if let Some(ops) = &stand.stand
                && !(0..=MAX_STAND_PRICE_CENTS).contains(&ops.price_cents)
            {
                return Some(format!("stand {} price is out of range", stand.id));
            }
        }
        None
    }
}

/// How much more often groups come while a campaign runs.
fn campaign_boost_percent() -> u32 {
    (ARRIVAL_MINUTES_PER_GUEST * 100 / CAMPAIGN_MINUTES_PER_GUEST).saturating_sub(100)
}
