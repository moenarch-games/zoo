//! Staffed food and drink stands (#102).
//!
//! A stand serves guests through the same errand machinery as the guest-comfort facilities
//! (#101): a guest's [`Visit`] names the stand and the need it meets, and the stand's `slots`
//! hold the guests being served. On top of that a stand keeps a first-come-first-served queue
//! of guests waiting at its counter, a number of posted vendors (each serves one guest at a
//! time), a finite stock and its own operating ledger.
//!
//! Guests buy in two ways. A guest walking past a stand buys on impulse once mildly hungry or
//! thirsty; a guest whose hunger or thirst crosses the detour threshold sets off for the stand
//! with the shortest expected wait it can reach, afford and queue at (route time plus the
//! queue ahead per vendor, where guests already on their way count as queueing, ties by ID). A
//! guest sees only the queue actually standing there: one that is full when it chooses, or
//! when it arrives, turns it away. Every minute each stand receives due deliveries, orders new
//! stock under its one resupply policy, advances the guests at its windows, starts serving the
//! queue in order and lets waiting guests' patience run down.
//!
//! The sale is the end of a guest's service time and happens exactly once, all at once: one
//! portion of stock, the guest's payment, the park's revenue, the stand's ledger, the guest's
//! recovered need and satisfaction, the wear on the stand and the wrapper the guest drops.
//! Ending a visit any other way leaves stock, money and needs as they were. When the stand
//! turned the guest away (closed, broken down, without a vendor, sold out with nothing on
//! order, a full queue, a price the guest cannot pay or thinks too dear (#106), or no patience
//! left) the stand records the lost sale and its reason; demolition and path cuts end the
//! errand without one.
//!
//! The resupply policy: with automatic restocking on, a stand at a quarter of its stock or less
//! orders a refill to full. The delivery arrives a fixed time later and is paid on arrival at
//! the stand's unit cost per portion it adds; a park that cannot pay receives nothing and the
//! stand orders again. Restocking can be ordered by hand at any time. Vendors cost only their
//! hourly wage, charged with the park's upkeep.
use super::{
    ActionResult, ConcessionKind, ExpenseCategory, GameState, GuestState, IncomeCategory,
    MAINTENANCE_FAILURE_THRESHOLD, MAINTENANCE_SERVICE_THRESHOLD, MAINTENANCE_WEAR_PER_SALE, Mover,
    Position,
    comfort::{self, Need, Resume, Visit},
    construction::dollars,
    content::{self, StandService},
    pricing,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// From this hunger a walking guest makes a detour to a food stand.
pub(crate) const FOOD_SEEK_HUNGER: u32 = 50;
/// From this thirst a walking guest makes a detour to a drink stand.
pub(crate) const DRINK_SEEK_THIRST: u32 = 50;
/// From this hunger or thirst a guest passing a stand buys on impulse, once per visit.
pub(crate) const IMPULSE_NEED: u32 = 20;
/// How long a guest waits in a stand's queue before giving up.
pub(crate) const PATIENCE_MINUTES: u32 = 15;
/// Minutes between ordering stock and its delivery.
pub(crate) const DELIVERY_MINUTES: u64 = 10;
/// A worn (degraded) stand serves this many minutes slower.
const DEGRADED_EXTRA_MINUTES: u32 = 2;
/// Guests walk one tile every three simulated minutes.
const MINUTES_PER_TILE: u32 = 3;
const LOST_SALE_HAPPINESS: u32 = 2;

/// The spending money guests arrived with before #106, for the schema 13 migration. Guests now
/// draw their whole budget at the gate (`pricing::member_budget`).
pub(crate) fn guest_budget(guest_id: u32) -> i64 {
    1_000 + i64::from(guest_id % 5) * 300
}

/// Why a sale was lost, counted per stand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LostDemand {
    /// The queue was full.
    pub queue_full: u32,
    /// The guest waited out its patience.
    pub gave_up: u32,
    /// The guest could not afford the price.
    pub unaffordable: u32,
    /// The price was above what the guest thought the item worth: a price complaint (#106).
    pub too_expensive: u32,
    /// The stand was closed by the player.
    pub closed: u32,
    /// The stand had no vendor.
    pub unstaffed: u32,
    /// The stand had no stock and none on order.
    pub out_of_stock: u32,
    /// The stand had broken down.
    pub broken_down: u32,
}

impl LostDemand {
    pub(crate) fn total(&self) -> u32 {
        [
            self.queue_full,
            self.gave_up,
            self.unaffordable,
            self.too_expensive,
            self.closed,
            self.unstaffed,
            self.out_of_stock,
            self.broken_down,
        ]
        .into_iter()
        .fold(0, u32::saturating_add)
    }

    fn count(&mut self, reason: LostReason) {
        let counter = match reason {
            LostReason::QueueFull => &mut self.queue_full,
            LostReason::GaveUp => &mut self.gave_up,
            LostReason::Unaffordable => &mut self.unaffordable,
            LostReason::TooExpensive => &mut self.too_expensive,
            LostReason::Closed => &mut self.closed,
            LostReason::Unstaffed => &mut self.unstaffed,
            LostReason::OutOfStock => &mut self.out_of_stock,
            LostReason::BrokenDown => &mut self.broken_down,
        };
        *counter = counter.saturating_add(1);
    }
}

/// What a guest can and will pay at a stand (#106).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Buyer {
    /// Spending money left.
    pub cash: i64,
    /// The most it pays, in percent of a stand's reference price.
    pub tolerance: u32,
}

/// The stand (by concession index) that turned a guest away and why, if any did.
pub(crate) type Refusal = Option<(usize, LostReason)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LostReason {
    QueueFull,
    GaveUp,
    Unaffordable,
    TooExpensive,
    Closed,
    Unstaffed,
    OutOfStock,
    BrokenDown,
}

/// One stand's operating figures over a period.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StandLedger {
    /// Completed sales.
    pub served: u32,
    pub revenue_cents: i64,
    /// Minutes the served guests spent queueing.
    pub wait_minutes: u64,
    pub stock_cost_cents: i64,
    pub wage_cents: i64,
    pub upkeep_cents: i64,
    pub repair_cents: i64,
    pub lost: LostDemand,
}

impl StandLedger {
    fn operating_cents(&self) -> i64 {
        self.stock_cost_cents
            .saturating_add(self.wage_cents)
            .saturating_add(self.upkeep_cents)
            .saturating_add(self.repair_cents)
    }

    fn add_expense(&mut self, line: StandExpense, cents: i64) {
        let field = match line {
            StandExpense::Stock => &mut self.stock_cost_cents,
            StandExpense::Wages => &mut self.wage_cents,
            StandExpense::Upkeep => &mut self.upkeep_cents,
            StandExpense::Repair => &mut self.repair_cents,
        };
        *field = field.saturating_add(cents);
    }

    /// Revenue less stock, wages, upkeep and repairs.
    pub(crate) fn contribution_cents(&self) -> i64 {
        self.revenue_cents.saturating_sub(self.operating_cents())
    }

    fn view(&self) -> StandLedgerView {
        let operating = self.operating_cents();
        StandLedgerView {
            served: self.served,
            revenue_cents: self.revenue_cents,
            average_wait_minutes: (self.served > 0)
                .then(|| self.wait_minutes / u64::from(self.served)),
            lost_sales: self.lost.total(),
            lost: self.lost,
            stock_cost_cents: self.stock_cost_cents,
            wage_cents: self.wage_cents,
            upkeep_cents: self.upkeep_cents,
            repair_cents: self.repair_cents,
            operating_cents: operating,
            contribution_cents: self.contribution_cents(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StandExpense {
    Stock,
    Wages,
    Upkeep,
    Repair,
}

/// A food or drink stand's operating state; comfort facilities have none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StandState {
    /// Whether the player has the stand open.
    pub open: bool,
    /// The player's price, `0..=pricing::MAX_STAND_PRICE_CENTS` (#106); a new stand starts at
    /// its definition's reference price.
    pub price_cents: i64,
    /// Vendors posted to the stand, `0..=max_vendors`.
    pub vendors: u32,
    pub stock: u32,
    /// Order a refill to full at a quarter of the stock or less.
    pub auto_restock: bool,
    /// The absolute minute the ordered delivery arrives; paid on arrival.
    pub delivery_due: Option<u64>,
    /// Guests waiting at the counter, first come first served.
    pub queue: Vec<u32>,
    pub today: StandLedger,
    pub total: StandLedger,
}

impl StandState {
    /// A new stand opens with one vendor, full stock and its reference price.
    pub(crate) fn new(service: &StandService, price_cents: i64) -> Self {
        Self {
            open: true,
            price_cents,
            vendors: 1,
            stock: service.stock_capacity,
            auto_restock: true,
            delivery_due: None,
            queue: Vec::new(),
            today: StandLedger::default(),
            total: StandLedger::default(),
        }
    }

    pub(crate) fn record_expense(&mut self, line: StandExpense, cents: i64) {
        self.today.add_expense(line, cents);
        self.total.add_expense(line, cents);
    }

    fn record_lost(&mut self, reason: LostReason) {
        self.today.lost.count(reason);
        self.total.lost.count(reason);
    }
}

/// Whether a stand takes guests now, and if not, why.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Availability {
    Open,
    /// Out of stock with a delivery on its way: guests may queue for it.
    Restocking,
    Closed,
    BrokenDown,
    Unstaffed,
    /// Out of stock with nothing on order.
    OutOfStock,
}

impl Availability {
    pub(crate) fn takes_guests(self) -> bool {
        matches!(self, Self::Open | Self::Restocking)
    }

    fn lost_reason(self) -> Option<LostReason> {
        match self {
            Self::Open | Self::Restocking => None,
            Self::Closed => Some(LostReason::Closed),
            Self::BrokenDown => Some(LostReason::BrokenDown),
            Self::Unstaffed => Some(LostReason::Unstaffed),
            Self::OutOfStock => Some(LostReason::OutOfStock),
        }
    }
}

/// A stand's service state for the inspector and workspace.
#[derive(Serialize)]
pub(crate) struct StandView {
    /// `open`, `restocking`, `closed`, `broken_down`, `unstaffed`, `out_of_stock` or
    /// `unreachable` (open, but no guest can walk to its counter from the entrance).
    status: &'static str,
    status_label: String,
    open: bool,
    /// The price now, and the authored reference price guests judge it against (#106).
    price_cents: i64,
    reference_price_cents: i64,
    vendors: u32,
    max_vendors: u32,
    vendor_hourly_wage_cents: i64,
    /// Guests at the windows now.
    serving: u32,
    /// Guests waiting at the counter.
    queue: u32,
    queue_capacity: u32,
    /// Guests walking here to queue.
    heading: u32,
    /// Minutes one sale takes now (slower while worn).
    service_minutes: u32,
    /// Sales the posted vendors can make in an hour at the current speed.
    capacity_per_hour: u32,
    stock: u32,
    stock_capacity: u32,
    /// The stock level at which automatic restocking orders a refill.
    restock_at: u32,
    auto_restock: bool,
    /// Minutes until the ordered delivery arrives.
    delivery_in_minutes: Option<u64>,
    /// What refilling the stock to full would cost now.
    refill_cost_cents: i64,
    stock_unit_cost_cents: i64,
    today: StandLedgerView,
    total: StandLedgerView,
}

#[derive(Serialize)]
struct StandLedgerView {
    served: u32,
    revenue_cents: i64,
    average_wait_minutes: Option<u64>,
    lost_sales: u32,
    lost: LostDemand,
    stock_cost_cents: i64,
    wage_cents: i64,
    upkeep_cents: i64,
    repair_cents: i64,
    operating_cents: i64,
    /// Revenue less stock, wages, upkeep and repairs.
    contribution_cents: i64,
}

impl Need {
    /// The stand kind that meets a need, if a stand meets it.
    pub(crate) fn stand(self) -> Option<ConcessionKind> {
        match self {
            Self::Food => Some(ConcessionKind::Food),
            Self::Drink => Some(ConcessionKind::Drink),
            Self::Toilet | Self::Rest => None,
        }
    }
}

fn restock_level(service: &StandService) -> u32 {
    service.stock_capacity / 4
}

impl GameState {
    fn stand_service(&self, index: usize) -> Option<&StandService> {
        self.concessions[index].definition.stand.as_ref()
    }

    /// Whether a stand takes guests now, and if not, why. Comfort facilities never do.
    pub(crate) fn stand_availability(&self, index: usize) -> Availability {
        let stand = &self.concessions[index];
        let Some(ops) = &stand.stand else {
            return Availability::Closed;
        };
        if !ops.open {
            Availability::Closed
        } else if stand.condition <= MAINTENANCE_FAILURE_THRESHOLD {
            Availability::BrokenDown
        } else if ops.vendors == 0 {
            Availability::Unstaffed
        } else if ops.stock == 0 && ops.delivery_due.is_some() {
            Availability::Restocking
        } else if ops.stock == 0 {
            Availability::OutOfStock
        } else {
            Availability::Open
        }
    }

    /// Minutes one sale takes at this stand now.
    fn service_minutes(&self, index: usize) -> u32 {
        let stand = &self.concessions[index];
        let base = self
            .stand_service(index)
            .map_or(1, |service| service.service_minutes);
        if stand.condition <= MAINTENANCE_SERVICE_THRESHOLD {
            base + DEGRADED_EXTRA_MINUTES
        } else {
            base
        }
    }

    /// Guests walking to each stand to queue, by concession index.
    fn heading_counts(&self) -> Vec<u32> {
        let mut counts = vec![0; self.concessions.len()];
        for guest in &self.guests {
            if guest.state != GuestState::WalkingToFacility {
                continue;
            }
            let Some(visit) = guest.visit.filter(|visit| visit.need.stand().is_some()) else {
                continue;
            };
            if let Some(index) = self.facility_position(visit.facility_id) {
                counts[index] += 1;
            }
        }
        counts
    }

    fn facility_position(&self, id: u32) -> Option<usize> {
        self.concessions.iter().position(|stand| stand.id == id)
    }

    fn guest_position_by_id(&self, guest_id: u32) -> Option<usize> {
        self.guests.iter().position(|guest| guest.id == guest_id)
    }

    fn record_lost(&mut self, index: usize, reason: LostReason) {
        if let Some(ops) = self.concessions[index].stand.as_mut() {
            ops.record_lost(reason);
        }
    }

    /// The stand a guest at `start` with `cash` should detour to for `need`: among the stands it
    /// can reach that take guests, that it can afford and whose queue is not full, the shortest
    /// expected wait (walking plus the queue ahead per vendor, counting the `heading` guests
    /// already on their way), then the lowest ID. Without one, the nearest reachable stand that
    /// turned the guest away and why.
    pub(crate) fn pick_stand(
        &self,
        need: Need,
        start: Position,
        buyer: Buyer,
        heading: &[u32],
    ) -> Result<(usize, Vec<Position>), Refusal> {
        let Some(kind) = need.stand() else {
            return Err(None);
        };
        let mut best: Option<((u32, u32), usize, Vec<Position>)> = None;
        let mut refused: Option<((usize, u32), usize, LostReason)> = None;
        for (index, stand) in self.concessions.iter().enumerate() {
            let (Some(ops), Some(service)) = (&stand.stand, self.stand_service(index)) else {
                continue;
            };
            if stand.kind != kind {
                continue;
            }
            let goals = self.concession_customer_tiles(stand.id);
            let Some(route) = self.path_to_any(Mover::Guest, start, &goals) else {
                continue;
            };
            let availability = self.stand_availability(index);
            let load = ops.queue.len() as u32 + heading[index];
            let reason = if let Some(reason) = availability.lost_reason() {
                Some(reason)
            } else if let Some(reason) = self.price_refusal(index, buyer) {
                Some(reason)
            } else if ops.queue.len() as u32 >= service.queue_capacity {
                Some(LostReason::QueueFull)
            } else {
                None
            };
            if let Some(reason) = reason {
                let key = (route.len(), stand.id);
                if refused.as_ref().is_none_or(|(best, ..)| key < *best) {
                    refused = Some((key, index, reason));
                }
                continue;
            }
            let walk = (route.len().saturating_sub(1) as u32).saturating_mul(MINUTES_PER_TILE);
            let ahead = load.saturating_add(stand.slots.len() as u32);
            let wait = ahead
                .saturating_mul(self.service_minutes(index))
                .div_ceil(ops.vendors.max(1));
            let key = (walk.saturating_add(wait), stand.id);
            if best.as_ref().is_none_or(|(best, ..)| key < *best) {
                best = Some((key, index, route));
            }
        }
        match best {
            Some((_, index, route)) => Ok((index, route)),
            None => Err(refused.map(|(_, index, reason)| (index, reason))),
        }
    }

    /// Sets a guest off for a stand chosen by [`GameState::pick_stand`].
    pub(crate) fn start_stand_visit(
        &mut self,
        guest_index: usize,
        stand_index: usize,
        need: Need,
        route: Vec<Position>,
        resume: Resume,
    ) {
        let facility_id = self.concessions[stand_index].id;
        let guest = &mut self.guests[guest_index];
        guest.visit = Some(Visit {
            facility_id,
            need,
            minutes: 0,
            resume,
            price_cents: 0,
        });
        guest.state = GuestState::WalkingToFacility;
        guest.route = route;
        guest.route_index = 0;
    }

    /// A refused detour: the stand that turned the guest away records the lost sale.
    pub(crate) fn refuse_stand(&mut self, guest_index: usize, refused: Refusal) {
        if let Some((index, reason)) = refused {
            self.record_lost(index, reason);
            self.price_complaint(guest_index, reason);
        }
    }

    /// The stand's price now.
    pub(crate) fn stand_price(&self, index: usize) -> i64 {
        let stand = &self.concessions[index];
        stand
            .stand
            .as_ref()
            .map_or(stand.definition.price_cents, |ops| ops.price_cents)
    }

    /// Why `buyer` will not pay the stand's price now, if it will not (#106): more than its
    /// spending money, or more than its tolerance of the stand's reference price.
    fn price_refusal(&self, index: usize, buyer: Buyer) -> Option<LostReason> {
        let price = self.stand_price(index);
        let reference = self.concessions[index].definition.price_cents;
        if price > buyer.cash {
            Some(LostReason::Unaffordable)
        } else if price > pricing::percent_of(reference, buyer.tolerance) {
            Some(LostReason::TooExpensive)
        } else {
            None
        }
    }

    fn buyer(&self, guest_index: usize) -> Buyer {
        let guest = &self.guests[guest_index];
        Buyer {
            cash: guest.cash_cents,
            tolerance: self.stand_tolerance(guest),
        }
    }

    /// A guest who found a price too steep thinks a little less of its visit.
    fn price_complaint(&mut self, guest_index: usize, reason: LostReason) {
        if reason == LostReason::TooExpensive {
            let guest = &mut self.guests[guest_index];
            guest.value_perception = guest
                .value_perception
                .saturating_sub(pricing::STAND_COMPLAINT_VALUE);
        }
    }

    /// Sets a stand's price (#106). Guests already at a window keep the price they agreed.
    pub(crate) fn set_stand_price(&mut self, stand_id: u32, cents: f64) -> ActionResult {
        let index = match self.stand_index_checked(stand_id) {
            Ok(index) => index,
            Err(error) => return error,
        };
        let label = self.stand_label(index);
        let cents = match pricing::validate_price(cents, pricing::MAX_STAND_PRICE_CENTS, "A price")
        {
            Ok(cents) => cents,
            Err(message) => return ActionResult::error(message),
        };
        let Some(ops) = self.concessions[index].stand.as_mut() else {
            return ActionResult::error("Select a food or drink stand first");
        };
        if ops.price_cents == cents {
            return ActionResult::ok(format!("{label} already charges {}", dollars(cents)));
        }
        let before = ops.price_cents;
        ops.price_cents = cents;
        ActionResult::ok(format!(
            "{label} price {} → {}",
            dollars(before),
            dollars(cents)
        ))
    }

    /// Walking guests beside a stand buy on impulse: a drink first, then food, each once per
    /// visit, from the first adjacent stand (in neighbour order) that takes guests, has room
    /// in its queue and that they can afford.
    pub(crate) fn join_passing_stands(&mut self) {
        for index in 0..self.guests.len() {
            let guest = &self.guests[index];
            let resume = match guest.state {
                GuestState::WalkingToHabitat => Resume::Habitat,
                GuestState::WalkingToExit => Resume::Exit,
                _ => continue,
            };
            let position = Position {
                x: guest.x,
                y: guest.y,
            };
            let mut wanted = Vec::new();
            if !guest.bought_drink && guest.thirst >= IMPULSE_NEED {
                wanted.push(Need::Drink);
            }
            if !guest.bought_food && guest.hunger >= IMPULSE_NEED {
                wanted.push(Need::Food);
            }
            if wanted.is_empty() {
                continue;
            }
            let buyer = self.buyer(index);
            let adjacent: Vec<usize> = self
                .neighbors(position)
                .into_iter()
                .filter_map(|tile| match self.tile(tile.x, tile.y) {
                    Some(super::TileKind::Concession(id)) => self.facility_position(id),
                    _ => None,
                })
                .collect();
            let choice = wanted.into_iter().find_map(|need| {
                adjacent.iter().copied().find_map(|stand_index| {
                    let stand = &self.concessions[stand_index];
                    let (Some(ops), Some(service)) =
                        (&stand.stand, self.stand_service(stand_index))
                    else {
                        return None;
                    };
                    let usable = need.stand() == Some(stand.kind)
                        && self.stand_availability(stand_index) == Availability::Open
                        && (ops.queue.len() as u32) < service.queue_capacity
                        && self.price_refusal(stand_index, buyer).is_none()
                        && self.concession_customer_tiles(stand.id).contains(&position);
                    usable.then_some((need, stand_index))
                })
            });
            let Some((need, stand_index)) = choice else {
                continue;
            };
            self.start_stand_visit(index, stand_index, need, Vec::new(), resume);
            self.arrive_at_stand(index);
        }
    }

    /// A guest reached a stand's counter: join the queue, or leave if it cannot take them.
    pub(crate) fn arrive_at_stand(&mut self, index: usize) {
        let Some(visit) = self.guests[index].visit else {
            return;
        };
        let Some(stand_index) = self.facility_position(visit.facility_id) else {
            self.end_visit(index, false);
            return;
        };
        let availability = self.stand_availability(stand_index);
        if let Some(reason) = availability.lost_reason() {
            self.lose_sale(index, stand_index, reason);
            return;
        }
        let capacity = self
            .stand_service(stand_index)
            .map_or(0, |service| service.queue_capacity);
        let queued = self.concessions[stand_index]
            .stand
            .as_ref()
            .map_or(u32::MAX, |ops| ops.queue.len() as u32);
        if queued >= capacity {
            self.lose_sale(index, stand_index, LostReason::QueueFull);
            return;
        }
        let guest_id = self.guests[index].id;
        if let Some(ops) = self.concessions[stand_index].stand.as_mut() {
            ops.queue.push(guest_id);
        }
        let guest = &mut self.guests[index];
        guest.route.clear();
        guest.route_index = 0;
        guest.state = GuestState::WaitingForFacility;
        if let Some(visit) = guest.visit.as_mut() {
            visit.minutes = PATIENCE_MINUTES;
        }
    }

    /// Ends a guest's visit without a sale and records why against the stand.
    fn lose_sale(&mut self, guest_index: usize, stand_index: usize, reason: LostReason) {
        self.record_lost(stand_index, reason);
        self.price_complaint(guest_index, reason);
        let guest = &mut self.guests[guest_index];
        guest.happiness = guest.happiness.saturating_sub(LOST_SALE_HAPPINESS);
        self.end_visit(guest_index, true);
    }

    /// One simulated minute at every stand: deliveries, restocking, service, the queue.
    pub(crate) fn advance_stands(&mut self) {
        let now = self.absolute_minute();
        for index in 0..self.concessions.len() {
            if self.concessions[index].stand.is_none() {
                continue;
            }
            self.receive_delivery(index, now);
            self.order_restock(index, now, false);
            self.advance_service(index);
            self.advance_queue(index);
        }
    }

    fn receive_delivery(&mut self, index: usize, now: u64) {
        let Some(service) = self.stand_service(index).copied() else {
            return;
        };
        let Some(ops) = self.concessions[index].stand.as_ref() else {
            return;
        };
        if ops.delivery_due.is_none_or(|due| due > now) {
            return;
        }
        let portions = service.stock_capacity.saturating_sub(ops.stock);
        let cost = i64::from(portions).saturating_mul(service.stock_unit_cost_cents);
        let paid = portions > 0 && self.spend(cost, ExpenseCategory::StandStock).is_ok();
        let Some(ops) = self.concessions[index].stand.as_mut() else {
            return;
        };
        ops.delivery_due = None;
        if paid {
            ops.stock = service.stock_capacity;
            ops.record_expense(StandExpense::Stock, cost);
        }
    }

    /// Orders a refill to full when automatic restocking is due, or when `by_hand`.
    fn order_restock(&mut self, index: usize, now: u64, by_hand: bool) -> bool {
        let Some(service) = self.stand_service(index).copied() else {
            return false;
        };
        let Some(ops) = self.concessions[index].stand.as_mut() else {
            return false;
        };
        let due = if by_hand {
            ops.stock < service.stock_capacity
        } else {
            ops.auto_restock && ops.stock <= restock_level(&service)
        };
        if !due || ops.delivery_due.is_some() {
            return false;
        }
        ops.delivery_due = Some(now + DELIVERY_MINUTES);
        true
    }

    /// Guests at the windows: a sale completes after the service time; a stand that broke down
    /// or lost its vendors stops serving. Closing lets service in progress finish.
    fn advance_service(&mut self, index: usize) {
        let availability = self.stand_availability(index);
        let service_minutes = self.service_minutes(index);
        let serving = self.concessions[index].slots.clone();
        for guest_id in serving {
            let Some(guest_index) = self.guest_position_by_id(guest_id) else {
                self.concessions[index]
                    .slots
                    .retain(|holder| *holder != guest_id);
                continue;
            };
            if let Some(reason) = match availability {
                Availability::BrokenDown => Some(LostReason::BrokenDown),
                Availability::Unstaffed => Some(LostReason::Unstaffed),
                _ => None,
            } {
                self.lose_sale(guest_index, index, reason);
                continue;
            }
            let guest = &mut self.guests[guest_index];
            let Some(visit) = guest.visit.as_mut() else {
                continue;
            };
            visit.minutes = visit.minutes.saturating_add(1);
            if visit.minutes >= service_minutes {
                self.complete_sale(index, guest_index);
            }
        }
    }

    /// The queue in order: unavailable stands send everyone away; otherwise free windows serve
    /// the first guests (who must be able to pay), and the rest lose a minute of patience.
    fn advance_queue(&mut self, index: usize) {
        let availability = self.stand_availability(index);
        let queue = self.concessions[index]
            .stand
            .as_ref()
            .map(|ops| ops.queue.clone())
            .unwrap_or_default();
        if let Some(reason) = availability.lost_reason() {
            for guest_id in queue {
                if let Some(guest_index) = self.guest_position_by_id(guest_id) {
                    self.lose_sale(guest_index, index, reason);
                }
            }
            return;
        }
        let mut waiting = Vec::new();
        for guest_id in queue {
            let Some(guest_index) = self.guest_position_by_id(guest_id) else {
                if let Some(ops) = self.concessions[index].stand.as_mut() {
                    ops.queue.retain(|holder| *holder != guest_id);
                }
                continue;
            };
            let stand = &self.concessions[index];
            let Some(ops) = &stand.stand else {
                return;
            };
            let window_free = (stand.slots.len() as u32) < ops.vendors
                && ops.stock > stand.slots.len() as u32
                && waiting.is_empty();
            if !window_free {
                waiting.push(guest_index);
                continue;
            }
            // The guest decides at the window and agrees today's price (#106).
            if let Some(reason) = self.price_refusal(index, self.buyer(guest_index)) {
                self.lose_sale(guest_index, index, reason);
                continue;
            }
            let price = self.stand_price(index);
            let waited = self.guests[guest_index]
                .visit
                .map_or(0, |visit| PATIENCE_MINUTES.saturating_sub(visit.minutes));
            let stand = &mut self.concessions[index];
            stand.slots.push(guest_id);
            if let Some(ops) = stand.stand.as_mut() {
                ops.queue.retain(|holder| *holder != guest_id);
                ops.today.wait_minutes = ops.today.wait_minutes.saturating_add(u64::from(waited));
                ops.total.wait_minutes = ops.total.wait_minutes.saturating_add(u64::from(waited));
            }
            let guest = &mut self.guests[guest_index];
            guest.state = GuestState::UsingFacility;
            if let Some(visit) = guest.visit.as_mut() {
                visit.minutes = 0;
                visit.price_cents = price;
            }
        }
        for guest_index in waiting {
            let Some(visit) = self.guests[guest_index].visit.as_mut() else {
                continue;
            };
            visit.minutes = visit.minutes.saturating_sub(1);
            if visit.minutes == 0 {
                self.lose_sale(guest_index, index, LostReason::GaveUp);
            }
        }
    }

    /// The sale: every effect of one purchase, applied together exactly once. If any part
    /// cannot be applied (no stock, no money, a full ledger), nothing is and the sale is lost.
    fn complete_sale(&mut self, index: usize, guest_index: usize) {
        let stand = &self.concessions[index];
        // The price agreed at the window (#106), whatever the stand charges now.
        let price = self.guests[guest_index]
            .visit
            .map_or(stand.definition.price_cents, |visit| visit.price_cents);
        let reference = stand.definition.price_cents;
        let kind = stand.kind;
        let stand_id = stand.id;
        let degraded = stand.condition <= MAINTENANCE_SERVICE_THRESHOLD;
        let Some(ops) = &stand.stand else {
            return;
        };
        let in_stock = ops.stock > 0;
        let revenue = stand
            .total_revenue_cents
            .checked_add(price)
            .filter(|total| *total <= content::MAX_CENTS);
        let affordable = self.guests[guest_index].cash_cents >= price;
        let countable = stand.total_sales < u32::MAX && stand.sales_today < u32::MAX;
        let Some(revenue) = revenue.filter(|_| in_stock && affordable && countable) else {
            let reason = if !in_stock {
                LostReason::OutOfStock
            } else {
                LostReason::Unaffordable
            };
            self.lose_sale(guest_index, index, reason);
            return;
        };
        if !self.earn(price, IncomeCategory::Concessions) {
            self.lose_sale(guest_index, index, LostReason::Unaffordable);
            return;
        }
        let stand = &mut self.concessions[index];
        stand.sales_today += 1;
        stand.total_sales += 1;
        stand.total_revenue_cents = revenue;
        stand.condition = stand.condition.saturating_sub(MAINTENANCE_WEAR_PER_SALE);
        stand
            .slots
            .retain(|holder| *holder != self.guests[guest_index].id);
        if let Some(ops) = stand.stand.as_mut() {
            ops.stock -= 1;
            for ledger in [&mut ops.today, &mut ops.total] {
                ledger.served = ledger.served.saturating_add(1);
                ledger.revenue_cents = ledger.revenue_cents.saturating_add(price);
            }
        }
        let now = self.absolute_minute();
        let guest = &mut self.guests[guest_index];
        let visit = guest.visit.take();
        guest.cash_cents -= price;
        let restore = if degraded { 35 } else { 55 };
        match kind {
            ConcessionKind::Food => {
                guest.hunger = guest.hunger.saturating_sub(restore);
                guest.bought_food = true;
            }
            ConcessionKind::Drink => {
                guest.thirst = guest.thirst.saturating_sub(restore);
                guest.bought_drink = true;
                guest.bladder = guest
                    .bladder
                    .saturating_add(comfort::DRINK_BLADDER)
                    .min(100);
            }
            ConcessionKind::Toilet
            | ConcessionKind::Bench
            | ConcessionKind::Bin
            | ConcessionKind::WaterPump
            | ConcessionKind::ClimatePlant
            | ConcessionKind::Sign => {}
        }
        let happiness = if degraded { 1 } else { 3 };
        guest.happiness = guest.happiness.saturating_add(happiness).min(100);
        // A fair price feels good; paying over the reference price does not (#106).
        if price <= reference {
            guest.value_perception = guest.value_perception.saturating_add(2).min(100);
        }
        if let Some(visit) = visit
            && guest.unmet_need == Some(visit.need)
        {
            guest.unmet_need = None;
        }
        guest.next_need_check = now;
        let position = Position {
            x: guest.x,
            y: guest.y,
        };
        self.ensure_maintenance_task(stand_id);
        self.dispose_litter(position);
        let resume = visit.map_or(Resume::Exit, |visit| visit.resume);
        self.resume_walk(guest_index, resume);
    }

    /// Removes a guest from every stand queue (slots are released with the comfort slots).
    pub(crate) fn release_stand_queue(&mut self, guest_id: u32) {
        for stand in &mut self.concessions {
            if let Some(ops) = stand.stand.as_mut() {
                ops.queue.retain(|holder| *holder != guest_id);
            }
        }
    }

    fn stand_index_checked(&self, stand_id: u32) -> Result<usize, ActionResult> {
        self.facility_position(stand_id)
            .filter(|index| self.concessions[*index].stand.is_some())
            .ok_or_else(|| ActionResult::error("Select a food or drink stand first"))
    }

    fn stand_label(&self, index: usize) -> String {
        let stand = &self.concessions[index];
        format!("{} #{}", stand.definition.label, stand.id)
    }

    /// Opens or closes a stand. Closing sends the queue away; service in progress finishes.
    pub(crate) fn set_stand_open(&mut self, stand_id: u32, open: bool) -> ActionResult {
        let index = match self.stand_index_checked(stand_id) {
            Ok(index) => index,
            Err(error) => return error,
        };
        let label = self.stand_label(index);
        let Some(ops) = self.concessions[index].stand.as_mut() else {
            return ActionResult::error("Select a food or drink stand first");
        };
        if ops.open == open {
            return ActionResult::ok(format!(
                "{label} is already {}",
                if open { "open" } else { "closed" }
            ));
        }
        ops.open = open;
        if !open {
            self.advance_queue(index);
        }
        ActionResult::ok(format!(
            "{label} {}",
            if open { "opened" } else { "closed" }
        ))
    }

    /// Posts `vendors` vendors at a stand; each costs the vendor wage every hour.
    pub(crate) fn set_stand_vendors(&mut self, stand_id: u32, vendors: u32) -> ActionResult {
        let index = match self.stand_index_checked(stand_id) {
            Ok(index) => index,
            Err(error) => return error,
        };
        let label = self.stand_label(index);
        let max = self
            .stand_service(index)
            .map_or(0, |service| service.max_vendors);
        if vendors > max {
            return ActionResult::error(format!("{label} has room for {max} vendors"));
        }
        let Some(ops) = self.concessions[index].stand.as_mut() else {
            return ActionResult::error("Select a food or drink stand first");
        };
        if ops.vendors == vendors {
            return ActionResult::ok(format!("{label} already has {vendors} vendors"));
        }
        ops.vendors = vendors;
        if vendors == 0 {
            self.advance_service(index);
            self.advance_queue(index);
        }
        ActionResult::ok(match vendors {
            0 => format!("{label} has no vendor now"),
            1 => format!("{label} now has 1 vendor"),
            count => format!("{label} now has {count} vendors"),
        })
    }

    /// Orders a refill to full by hand; paid when it arrives.
    pub(crate) fn restock_stand(&mut self, stand_id: u32) -> ActionResult {
        let index = match self.stand_index_checked(stand_id) {
            Ok(index) => index,
            Err(error) => return error,
        };
        let label = self.stand_label(index);
        let now = self.absolute_minute();
        if self.order_restock(index, now, true) {
            return ActionResult::ok(format!(
                "{label} restock ordered · arrives in {DELIVERY_MINUTES} min"
            ));
        }
        let pending = self.concessions[index]
            .stand
            .as_ref()
            .is_some_and(|ops| ops.delivery_due.is_some());
        ActionResult::ok(if pending {
            format!("{label} already has a delivery on its way")
        } else {
            format!("{label} is fully stocked")
        })
    }

    pub(crate) fn set_stand_auto_restock(&mut self, stand_id: u32, auto: bool) -> ActionResult {
        let index = match self.stand_index_checked(stand_id) {
            Ok(index) => index,
            Err(error) => return error,
        };
        let label = self.stand_label(index);
        let Some(ops) = self.concessions[index].stand.as_mut() else {
            return ActionResult::error("Select a food or drink stand first");
        };
        if ops.auto_restock == auto {
            return ActionResult::ok(format!(
                "{label} automatic restocking is already {}",
                if auto { "on" } else { "off" }
            ));
        }
        ops.auto_restock = auto;
        ActionResult::ok(format!(
            "{label} automatic restocking {}",
            if auto { "on" } else { "off" }
        ))
    }

    /// Hourly vendor wages per stand, in concession order.
    pub(crate) fn vendor_wages(&self) -> Vec<i64> {
        let wage = self.catalog.balance.staff.vendor.hourly_wage_cents;
        self.concessions
            .iter()
            .map(|stand| {
                stand
                    .stand
                    .as_ref()
                    .map_or(0, |ops| i64::from(ops.vendors).saturating_mul(wage))
            })
            .collect()
    }

    /// Why the stand turns guests away, `None` when it takes them; for alerts and inspectors.
    pub(crate) fn stand_status(&self, index: usize) -> &'static str {
        match self.stand_availability(index) {
            Availability::Open if !self.stand_reachable(index) => "unreachable",
            Availability::Open => "open",
            Availability::Restocking => "restocking",
            Availability::Closed => "closed",
            Availability::BrokenDown => "broken_down",
            Availability::Unstaffed => "unstaffed",
            Availability::OutOfStock => "out_of_stock",
        }
    }

    /// Whether a guest can walk from the entrance to the stand's counter. Presentation: an
    /// uncounted search.
    fn stand_reachable(&self, index: usize) -> bool {
        let goals = self.concession_customer_tiles(self.concessions[index].id);
        self.search_path(Mover::Guest, self.entrance, &goals)
            .is_some()
    }

    pub(crate) fn stand_view(&self, index: usize, heading: &[u32]) -> Option<StandView> {
        let stand = &self.concessions[index];
        let (ops, service) = (stand.stand.as_ref()?, self.stand_service(index)?);
        let status = self.stand_status(index);
        let service_minutes = self.service_minutes(index);
        let now = self.absolute_minute();
        let queue = ops.queue.len() as u32;
        let status_label = match status {
            "open" if queue > 0 => format!(
                "Open · {queue} in queue{}",
                if queue >= service.queue_capacity {
                    " (full)"
                } else {
                    ""
                }
            ),
            "open" => "Open".to_owned(),
            "unreachable" => "Open, but no guest path reaches the counter".to_owned(),
            "restocking" => "Sold out · delivery on its way".to_owned(),
            "closed" => "Closed".to_owned(),
            "broken_down" => "Closed · broken down".to_owned(),
            "unstaffed" => "Closed · no vendor".to_owned(),
            _ => "Closed · out of stock".to_owned(),
        };
        let refill = service.stock_capacity.saturating_sub(ops.stock);
        Some(StandView {
            status,
            status_label,
            open: ops.open,
            price_cents: ops.price_cents,
            reference_price_cents: stand.definition.price_cents,
            vendors: ops.vendors,
            max_vendors: service.max_vendors,
            vendor_hourly_wage_cents: self.catalog.balance.staff.vendor.hourly_wage_cents,
            serving: stand.slots.len() as u32,
            queue,
            queue_capacity: service.queue_capacity,
            heading: heading.get(index).copied().unwrap_or(0),
            service_minutes,
            capacity_per_hour: (60 / service_minutes.max(1)).saturating_mul(ops.vendors),
            stock: ops.stock,
            stock_capacity: service.stock_capacity,
            restock_at: restock_level(service),
            auto_restock: ops.auto_restock,
            delivery_in_minutes: ops.delivery_due.map(|due| due.saturating_sub(now)),
            refill_cost_cents: i64::from(refill).saturating_mul(service.stock_unit_cost_cents),
            stock_unit_cost_cents: service.stock_unit_cost_cents,
            today: ops.today.view(),
            total: ops.total.view(),
        })
    }

    pub(crate) fn stand_heading_counts(&self) -> Vec<u32> {
        self.heading_counts()
    }

    /// Stand consistency (#102), checked with the other per-tick invariants and on load: only
    /// stands have operating state, queues and windows hold distinct guests who are waiting or
    /// being served there, vendors and stock stay within the stand's definition, and the ledger
    /// agrees with the sale counters.
    pub(crate) fn stand_violation(&self) -> Option<String> {
        let mut seen = HashSet::new();
        for stand in &self.concessions {
            let Some(ops) = &stand.stand else {
                if stand.kind.sells() {
                    return Some(format!("stand {} has no operating state", stand.id));
                }
                continue;
            };
            let Some(service) = &stand.definition.stand else {
                return Some(format!("stand {} has no service model", stand.id));
            };
            if !stand.kind.sells() {
                return Some(format!("facility {} has stand state", stand.id));
            }
            if ops.vendors > service.max_vendors
                || ops.stock > service.stock_capacity
                || ops.queue.len() as u32 > service.queue_capacity
                || stand.slots.len() as u32 > service.max_vendors
            {
                return Some(format!("stand {} is outside its service model", stand.id));
            }
            if ops.stock < stand.slots.len() as u32 {
                return Some(format!(
                    "stand {} serves more guests than it has stock for",
                    stand.id
                ));
            }
            if ops.total.served != stand.total_sales
                || ops.total.revenue_cents != stand.total_revenue_cents
                || ops.today.served != stand.sales_today
            {
                return Some(format!(
                    "stand {} ledger disagrees with its sales",
                    stand.id
                ));
            }
            for (holders, state) in [
                (&ops.queue, GuestState::WaitingForFacility),
                (&stand.slots, GuestState::UsingFacility),
            ] {
                for holder in holders {
                    if !seen.insert(*holder) {
                        return Some(format!("guest {holder} is in two stand places"));
                    }
                    let bound = self.guests.iter().any(|guest| {
                        guest.id == *holder
                            && guest.state == state
                            && guest
                                .visit
                                .is_some_and(|visit| visit.facility_id == stand.id)
                    });
                    if !bound {
                        return Some(format!(
                            "stand {} holds a place for guest {holder} who is not there",
                            stand.id
                        ));
                    }
                }
            }
        }
        for guest in &self.guests {
            if guest.cash_cents < 0 {
                return Some(format!("guest {} has negative cash", guest.id));
            }
            let Some(visit) = guest.visit.filter(|visit| visit.need.stand().is_some()) else {
                continue;
            };
            let placed = match guest.state {
                GuestState::WaitingForFacility | GuestState::UsingFacility => {
                    seen.contains(&guest.id)
                }
                _ => !seen.contains(&guest.id),
            };
            if !placed {
                return Some(format!(
                    "guest {} at stand {} is not in its queue or window",
                    guest.id, visit.facility_id
                ));
            }
        }
        None
    }
}
