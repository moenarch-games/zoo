//! Budgets, loans, financial history and insolvency recovery (#115).
//!
//! Money stays whole cents in `zoo-core`; the browser only lays these views out and sends the
//! borrow and repay commands.
//!
//! **Cash flows.** Every change to park cash goes through the categorized day ledger (#39).
//! Its lines fall into three kinds: *operating* (admissions and concessions against wages,
//! upkeep, supplies, repairs, hiring and marketing), *investment* (construction, land, animal
//! purchases and utility replacements) and *financing* (loan proceeds, principal repayments and
//! interest). Loan proceeds are never income and principal repayments are never an expense, so
//! the operating result shows whether the park itself earns money while the financing lines
//! show how much cash borrowing provided. Each day closes with its opening and closing cash, and
//! `closing = opening + income − expenses + proceeds − repayments` holds exactly.
//!
//! **History.** The last [`HISTORY_DAYS`] completed days are kept (older days are dropped), and
//! 7- and 30-day summaries are computed from them on request.
//!
//! **Loans.** A scenario may offer one credit line: a borrowing limit and a daily interest rate
//! in basis points. Borrowing adds the amount to cash and to the balance owed, up to the limit;
//! repaying takes it from cash (never more than the cash on hand or the balance owed). There is
//! no term: the balance is owed until repaid. Interest is charged once per day, at the day
//! boundary, on the *highest* balance owed during that day, rounded up to a whole cent, so
//! borrowing and repaying within a day costs a day's interest instead of nothing and repeated
//! commands can neither mint money nor charge interest twice. A scenario may start a park with
//! debt already owed.
//!
//! **Insolvency (Atlas Decision 6.1.6).** A park whose cash covers less than a day of its
//! recurring commitments is *low on cash* (a warning); a park below zero is *in deficit* (a
//! critical alert). Nothing ends the game or resets the park, and nothing is forgiven: in deficit
//! every purchase, construction and hire is refused for want of cash while wages, upkeep and
//! interest keep accruing, and every day that closes in deficit is counted for later scoring.
//! Recovery uses the normal rules: prices, vendors, closing stands, rehoming animals and
//! postponing expansion, with the scenario's remaining credit as the only, finite, bridge.
use super::{
    ActionResult, ExpenseCategory, FinanceLedger, GameState, Position,
    alerts::{AlertAction, AlertCode, Severity, Subject},
    construction::dollars,
    content,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Completed days kept in the history.
pub(crate) const HISTORY_DAYS: usize = 30;
/// The summary periods, in days.
const PERIODS: [usize; 2] = [7, 30];
/// Completed days the budget projection averages.
const BASIS_DAYS: usize = 7;
/// The highest daily interest rate a scenario may set: 5 % a day.
pub(crate) const MAX_DAILY_INTEREST_BASIS_POINTS: u32 = 500;
const BASIS_POINTS: i128 = 10_000;
/// The largest expense lines shown as cost drivers.
const DRIVERS: usize = 3;

/// A scenario's credit line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoanTerms {
    /// The most the park may owe at once.
    pub limit_cents: i64,
    /// Interest per day on the day's highest balance, in hundredths of a percent.
    pub daily_interest_basis_points: u32,
}

/// The credit line as a scenario authors it: its terms and any debt the park starts with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoanDefinition {
    pub limit_cents: i64,
    pub daily_interest_basis_points: u32,
    #[serde(default)]
    pub starting_balance_cents: i64,
}

impl LoanDefinition {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(1..=content::MAX_CENTS / 2).contains(&self.limit_cents) {
            return Err(format!(
                "loan/limit_cents: expected 1..={}",
                content::MAX_CENTS / 2
            ));
        }
        if self.daily_interest_basis_points > MAX_DAILY_INTEREST_BASIS_POINTS {
            return Err(format!(
                "loan/daily_interest_basis_points: expected 0..={MAX_DAILY_INTEREST_BASIS_POINTS}"
            ));
        }
        if !(0..=self.limit_cents).contains(&self.starting_balance_cents) {
            return Err("loan/starting_balance_cents: expected 0..=limit_cents".to_owned());
        }
        Ok(())
    }

    pub(crate) fn terms(&self) -> LoanTerms {
        LoanTerms {
            limit_cents: self.limit_cents,
            daily_interest_basis_points: self.daily_interest_basis_points,
        }
    }
}

/// The park's credit line and what it owes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Loan {
    /// `None` when the scenario offers no credit.
    pub terms: Option<LoanTerms>,
    pub balance_cents: i64,
    /// The highest balance owed since the last interest charge; tonight's interest base.
    pub peak_cents: i64,
    pub borrowed_total_cents: i64,
    pub repaid_total_cents: i64,
    pub interest_total_cents: i64,
}

impl Loan {
    /// A park's credit line under `terms`, owing `balance` from the start.
    pub(crate) fn new(terms: Option<LoanTerms>, balance_cents: i64) -> Self {
        Self {
            terms,
            balance_cents,
            peak_cents: balance_cents,
            ..Self::default()
        }
    }

    fn headroom_cents(&self) -> i64 {
        self.terms
            .map_or(0, |terms| (terms.limit_cents - self.balance_cents).max(0))
    }

    fn rate(&self) -> u32 {
        self.terms
            .map_or(0, |terms| terms.daily_interest_basis_points)
    }
}

/// A day's interest on `balance`: `balance × rate / 10 000`, rounded up to a whole cent.
pub(crate) fn daily_interest_cents(balance_cents: i64, basis_points: u32) -> i64 {
    let numerator = i128::from(balance_cents.max(0)) * i128::from(basis_points);
    let interest = (numerator + BASIS_POINTS - 1) / BASIS_POINTS;
    i64::try_from(interest).unwrap_or(i64::MAX)
}

/// One completed day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DayRecord {
    pub day: u32,
    pub opening_cash_cents: i64,
    pub closing_cash_cents: i64,
    /// What the park owed when the day closed.
    pub loan_balance_cents: i64,
    pub ledger: FinanceLedger,
}

/// Saved financial state beyond today's ledger.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Finances {
    /// Cash when today began.
    pub opening_cash_cents: i64,
    /// The last [`HISTORY_DAYS`] completed days, oldest first.
    pub history: VecDeque<DayRecord>,
    pub loan: Loan,
    /// Days that closed with negative cash, ever and in a row up to the last closed day.
    pub deficit_days: u32,
    pub deficit_streak_days: u32,
}

/// How a ledger line counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineKind {
    Operating,
    /// Charged every hour from what the park has now (wages, upkeep, running plants).
    Recurring,
    Investment,
    Interest,
}

impl FinanceLedger {
    /// Every expense line with its saved field name and kind.
    fn expense_lines(self) -> [(&'static str, LineKind, i64); 22] {
        use LineKind::{Interest, Investment, Operating, Recurring};
        [
            (
                "construction_expense_cents",
                Investment,
                self.construction_expense_cents,
            ),
            (
                "animal_purchase_expense_cents",
                Investment,
                self.animal_purchase_expense_cents,
            ),
            (
                "habitat_care_expense_cents",
                Operating,
                self.habitat_care_expense_cents,
            ),
            (
                "animal_feed_expense_cents",
                Operating,
                self.animal_feed_expense_cents,
            ),
            (
                "keeper_hiring_expense_cents",
                Operating,
                self.keeper_hiring_expense_cents,
            ),
            (
                "janitor_hiring_expense_cents",
                Operating,
                self.janitor_hiring_expense_cents,
            ),
            (
                "mechanic_hiring_expense_cents",
                Operating,
                self.mechanic_hiring_expense_cents,
            ),
            (
                "maintenance_repair_expense_cents",
                Operating,
                self.maintenance_repair_expense_cents,
            ),
            (
                "park_upkeep_expense_cents",
                Recurring,
                self.park_upkeep_expense_cents,
            ),
            (
                "keeper_wages_expense_cents",
                Recurring,
                self.keeper_wages_expense_cents,
            ),
            (
                "janitor_wages_expense_cents",
                Recurring,
                self.janitor_wages_expense_cents,
            ),
            (
                "mechanic_wages_expense_cents",
                Recurring,
                self.mechanic_wages_expense_cents,
            ),
            ("land_expense_cents", Investment, self.land_expense_cents),
            (
                "vendor_wages_expense_cents",
                Recurring,
                self.vendor_wages_expense_cents,
            ),
            (
                "stand_stock_expense_cents",
                Operating,
                self.stand_stock_expense_cents,
            ),
            (
                "marketing_expense_cents",
                Operating,
                self.marketing_expense_cents,
            ),
            (
                "vet_hiring_expense_cents",
                Operating,
                self.vet_hiring_expense_cents,
            ),
            (
                "vet_wages_expense_cents",
                Recurring,
                self.vet_wages_expense_cents,
            ),
            (
                "medicine_expense_cents",
                Operating,
                self.medicine_expense_cents,
            ),
            (
                "utility_operation_expense_cents",
                Recurring,
                self.utility_operation_expense_cents,
            ),
            (
                "utility_replacement_expense_cents",
                Investment,
                self.utility_replacement_expense_cents,
            ),
            (
                "loan_interest_expense_cents",
                Interest,
                self.loan_interest_expense_cents,
            ),
        ]
    }

    fn sum(self, test: impl Fn(LineKind) -> bool) -> i64 {
        self.expense_lines()
            .iter()
            .filter(|(_, kind, _)| test(*kind))
            .fold(0_i64, |total, (_, _, cents)| total.saturating_add(*cents))
    }

    fn operating_expense_cents(self) -> i64 {
        self.sum(|kind| matches!(kind, LineKind::Operating | LineKind::Recurring))
    }

    fn investment_cents(self) -> i64 {
        self.sum(|kind| kind == LineKind::Investment)
    }

    /// Supplies, care and repairs: the operating costs that follow from running the park,
    /// rather than from a one-off decision such as hiring or marketing.
    fn supplies_cents(self) -> i64 {
        self.habitat_care_expense_cents
            .saturating_add(self.animal_feed_expense_cents)
            .saturating_add(self.maintenance_repair_expense_cents)
            .saturating_add(self.stand_stock_expense_cents)
            .saturating_add(self.medicine_expense_cents)
    }

    /// Records borrowed cash or a principal repayment; neither is income or expense.
    pub(crate) fn record_financing(&mut self, repayment: bool, cents: i64) -> bool {
        let field = if repayment {
            &mut self.loan_repayment_cents
        } else {
            &mut self.loan_proceeds_cents
        };
        match field
            .checked_add(cents)
            .filter(|total| cents >= 0 && *total <= content::MAX_CENTS / 2)
        {
            Some(total) => {
                *field = total;
                true
            }
            None => false,
        }
    }

    /// The sum of two ledgers, line by line.
    fn plus(self, other: Self) -> Self {
        let add = |a: i64, b: i64| a.saturating_add(b);
        Self {
            admissions_income_cents: add(
                self.admissions_income_cents,
                other.admissions_income_cents,
            ),
            concession_income_cents: add(
                self.concession_income_cents,
                other.concession_income_cents,
            ),
            construction_expense_cents: add(
                self.construction_expense_cents,
                other.construction_expense_cents,
            ),
            animal_purchase_expense_cents: add(
                self.animal_purchase_expense_cents,
                other.animal_purchase_expense_cents,
            ),
            habitat_care_expense_cents: add(
                self.habitat_care_expense_cents,
                other.habitat_care_expense_cents,
            ),
            animal_feed_expense_cents: add(
                self.animal_feed_expense_cents,
                other.animal_feed_expense_cents,
            ),
            keeper_hiring_expense_cents: add(
                self.keeper_hiring_expense_cents,
                other.keeper_hiring_expense_cents,
            ),
            janitor_hiring_expense_cents: add(
                self.janitor_hiring_expense_cents,
                other.janitor_hiring_expense_cents,
            ),
            mechanic_hiring_expense_cents: add(
                self.mechanic_hiring_expense_cents,
                other.mechanic_hiring_expense_cents,
            ),
            maintenance_repair_expense_cents: add(
                self.maintenance_repair_expense_cents,
                other.maintenance_repair_expense_cents,
            ),
            park_upkeep_expense_cents: add(
                self.park_upkeep_expense_cents,
                other.park_upkeep_expense_cents,
            ),
            keeper_wages_expense_cents: add(
                self.keeper_wages_expense_cents,
                other.keeper_wages_expense_cents,
            ),
            janitor_wages_expense_cents: add(
                self.janitor_wages_expense_cents,
                other.janitor_wages_expense_cents,
            ),
            mechanic_wages_expense_cents: add(
                self.mechanic_wages_expense_cents,
                other.mechanic_wages_expense_cents,
            ),
            land_expense_cents: add(self.land_expense_cents, other.land_expense_cents),
            vendor_wages_expense_cents: add(
                self.vendor_wages_expense_cents,
                other.vendor_wages_expense_cents,
            ),
            stand_stock_expense_cents: add(
                self.stand_stock_expense_cents,
                other.stand_stock_expense_cents,
            ),
            marketing_expense_cents: add(
                self.marketing_expense_cents,
                other.marketing_expense_cents,
            ),
            vet_hiring_expense_cents: add(
                self.vet_hiring_expense_cents,
                other.vet_hiring_expense_cents,
            ),
            vet_wages_expense_cents: add(
                self.vet_wages_expense_cents,
                other.vet_wages_expense_cents,
            ),
            medicine_expense_cents: add(self.medicine_expense_cents, other.medicine_expense_cents),
            utility_operation_expense_cents: add(
                self.utility_operation_expense_cents,
                other.utility_operation_expense_cents,
            ),
            utility_replacement_expense_cents: add(
                self.utility_replacement_expense_cents,
                other.utility_replacement_expense_cents,
            ),
            loan_interest_expense_cents: add(
                self.loan_interest_expense_cents,
                other.loan_interest_expense_cents,
            ),
            loan_proceeds_cents: add(self.loan_proceeds_cents, other.loan_proceeds_cents),
            loan_repayment_cents: add(self.loan_repayment_cents, other.loan_repayment_cents),
        }
    }

    /// Net change in cash the ledger explains.
    pub(crate) fn net_cash_flow_cents(self) -> i64 {
        self.profit_cents()
            .saturating_add(self.loan_proceeds_cents)
            .saturating_sub(self.loan_repayment_cents)
    }
}

/// What the cash on hand means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CashStatus {
    Healthy,
    /// Less than a day of recurring commitments left.
    Low,
    /// Below zero: purchases wait until cash is positive again.
    Deficit,
}

/// Whether the park's own operations or borrowing paid for a period.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Verdict {
    /// Operations earned more than they cost.
    OperationsProfitable,
    /// Operations lost money and net borrowing covered at least part of it.
    BorrowingCoversLosses,
    /// Operations lost money and cash on hand paid for it.
    OperationsLosing,
    NoActivity,
}

/// One ledger's flows by kind.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Flows {
    /// Operating income (admissions and concessions).
    pub income_cents: i64,
    /// Operating, investment and interest expenses; never principal.
    pub expenses_cents: i64,
    /// `income − expenses`.
    pub profit_cents: i64,
    pub operating_expenses_cents: i64,
    pub operating_result_cents: i64,
    pub investment_cents: i64,
    pub interest_cents: i64,
    pub loan_proceeds_cents: i64,
    pub loan_repayments_cents: i64,
    /// Proceeds less repayments: cash borrowing provided.
    pub net_borrowing_cents: i64,
    pub net_cash_flow_cents: i64,
    pub verdict: Verdict,
    pub breakdown: FinanceLedger,
}

impl Flows {
    pub(crate) fn new(ledger: FinanceLedger) -> Self {
        let operating_expenses_cents = ledger.operating_expense_cents();
        let operating_result_cents = ledger
            .income_total_cents()
            .saturating_sub(operating_expenses_cents);
        let net_borrowing_cents = ledger
            .loan_proceeds_cents
            .saturating_sub(ledger.loan_repayment_cents);
        let verdict = if operating_result_cents > 0 {
            Verdict::OperationsProfitable
        } else if operating_result_cents < 0 && net_borrowing_cents > 0 {
            Verdict::BorrowingCoversLosses
        } else if operating_result_cents < 0 {
            Verdict::OperationsLosing
        } else {
            Verdict::NoActivity
        };
        Self {
            income_cents: ledger.income_total_cents(),
            expenses_cents: ledger.expense_total_cents(),
            profit_cents: ledger.profit_cents(),
            operating_expenses_cents,
            operating_result_cents,
            investment_cents: ledger.investment_cents(),
            interest_cents: ledger.loan_interest_expense_cents,
            loan_proceeds_cents: ledger.loan_proceeds_cents,
            loan_repayments_cents: ledger.loan_repayment_cents,
            net_borrowing_cents,
            net_cash_flow_cents: ledger.net_cash_flow_cents(),
            verdict,
            breakdown: ledger,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct FinanceDayView {
    pub day: u32,
    pub opening_cash_cents: i64,
    /// Cash at the close, or now for today.
    pub closing_cash_cents: i64,
    pub loan_balance_cents: i64,
    #[serde(flatten)]
    pub flows: Flows,
}

impl FinanceDayView {
    fn of(record: &DayRecord) -> Self {
        Self {
            day: record.day,
            opening_cash_cents: record.opening_cash_cents,
            closing_cash_cents: record.closing_cash_cents,
            loan_balance_cents: record.loan_balance_cents,
            flows: Flows::new(record.ledger),
        }
    }
}

/// The last `days` completed days together.
#[derive(Serialize)]
struct PeriodView {
    /// The period asked for, and how many completed days it actually covers.
    period_days: usize,
    days: usize,
    first_day: Option<u32>,
    last_day: Option<u32>,
    opening_cash_cents: Option<i64>,
    closing_cash_cents: Option<i64>,
    flows: Flows,
}

#[derive(Serialize)]
struct LoanView {
    available: bool,
    limit_cents: i64,
    daily_interest_basis_points: u32,
    balance_cents: i64,
    /// What more may be borrowed now.
    headroom_cents: i64,
    /// Interest charged at tonight's day boundary on the day's highest balance so far.
    interest_tonight_cents: i64,
    /// A day's interest at the current balance.
    interest_per_day_cents: i64,
    borrowed_total_cents: i64,
    repaid_total_cents: i64,
    interest_total_cents: i64,
}

#[derive(Serialize)]
struct Line {
    line: &'static str,
    cents: i64,
}

/// What the park is committed to and what it may earn, with the assumptions stated.
#[derive(Serialize)]
struct BudgetView {
    /// Recurring hourly charges for 24 hours, by ledger line, from what the park has now.
    commitments: Vec<Line>,
    /// Those charges plus a day's interest at the current balance.
    committed_per_day_cents: i64,
    /// Completed days the income and supplies estimates average (at most 7).
    basis_days: usize,
    expected_income_per_day_cents: i64,
    expected_supplies_per_day_cents: i64,
    projected_net_per_day_cents: i64,
    /// Whole days cash lasts at the projected rate; `None` while it is not running down.
    runway_days: Option<i64>,
    /// The largest expense lines over the last 7 completed days and today.
    drivers: Vec<Line>,
    assumptions: Vec<&'static str>,
}

/// A practical step out of a deficit, with what it is worth where that is known.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RecoveryStep {
    /// Admission is below what guests think the park is worth.
    RaiseAdmission {
        admission_cents: i64,
        fair_cents: i64,
    },
    /// A stand that lost money today; its vendors cost this much a day.
    CutVendors {
        stand_id: u32,
        vendors: u32,
        wages_per_day_cents: i64,
        margin_today_cents: i64,
    },
    /// Construction, land and animals bought over the last 7 days and today.
    PostponeExpansion { invested_cents: i64 },
    /// What the animals' upkeep costs a day; rehoming lowers it (with no refund).
    RehomeAnimals { animal_upkeep_per_day_cents: i64 },
    /// Credit still available as a bridge.
    Borrow { headroom_cents: i64 },
}

#[derive(Serialize)]
struct SolvencyView {
    status: CashStatus,
    /// Cash below this is low: a day of recurring commitments.
    low_cash_line_cents: i64,
    deficit_cents: i64,
    deficit_days: u32,
    deficit_streak_days: u32,
    /// Purchases, building and hiring wait while the park is in deficit.
    spending_blocked: bool,
    recovery: Vec<RecoveryStep>,
}

/// The finances report (#39, #115).
#[derive(Serialize)]
pub(crate) struct FinanceView {
    admission_price_cents: i64,
    pub current_day: FinanceDayView,
    pub previous_day: Option<FinanceDayView>,
    pub profit_change_cents: Option<i64>,
    pub profit_trend: &'static str,
    solvency: SolvencyView,
    loan: LoanView,
    budget: BudgetView,
    periods: Vec<PeriodView>,
    /// The kept completed days, newest first.
    history: Vec<FinanceDayView>,
}

/// A budget plan: candidate spending and borrowing against the projection. Changes nothing.
#[derive(Serialize)]
pub(crate) struct BudgetPlan {
    spend_cents: i64,
    borrow_cents: i64,
    cash_after_cents: i64,
    /// The spending fits in the cash the plan leaves.
    affordable: bool,
    /// The borrowing fits under the limit.
    within_limit: bool,
    loan_balance_after_cents: i64,
    interest_per_day_after_cents: i64,
    projected_net_per_day_cents: i64,
    runway_days: Option<i64>,
}

fn runway(cash: i64, net_per_day: i64) -> Option<i64> {
    if net_per_day >= 0 {
        None
    } else if cash <= 0 {
        Some(0)
    } else {
        Some(cash / net_per_day.saturating_neg())
    }
}

impl GameState {
    /// Borrows whole cents from the scenario's credit line, up to its limit.
    pub(crate) fn borrow_loan(&mut self, cents: f64) -> ActionResult {
        let Some(terms) = self.finance.loan.terms else {
            return ActionResult::error("This scenario offers no loans");
        };
        let cents = match super::pricing::validate_price(cents, terms.limit_cents, "A loan") {
            Ok(0) => return ActionResult::error("Borrow more than $0"),
            Ok(cents) => cents,
            Err(message) => return ActionResult::error(message),
        };
        let headroom = self.finance.loan.headroom_cents();
        if cents > headroom {
            return ActionResult::error(format!(
                "The limit is {}: only {} more can be borrowed",
                dollars(terms.limit_cents),
                dollars(headroom)
            ));
        }
        let Some(cash) = self
            .cash_cents
            .checked_add(cents)
            .filter(|cash| *cash <= content::MAX_CENTS)
        else {
            return ActionResult::error("Cash exceeds the exact financial range");
        };
        let mut ledger = self.finance_today;
        if !ledger.record_financing(false, cents) {
            return ActionResult::error("Borrowing exceeds the exact financial range");
        }
        let loan = &mut self.finance.loan;
        loan.balance_cents += cents;
        loan.peak_cents = loan.peak_cents.max(loan.balance_cents);
        loan.borrowed_total_cents = loan.borrowed_total_cents.saturating_add(cents);
        self.cash_cents = cash;
        self.finance_today = ledger;
        self.emit("loan_borrowed", None);
        ActionResult::ok(format!(
            "Borrowed {}; the park owes {}",
            dollars(cents),
            dollars(self.finance.loan.balance_cents)
        ))
    }

    /// Repays whole cents of the balance owed from cash on hand.
    pub(crate) fn repay_loan(&mut self, cents: f64) -> ActionResult {
        let owed = self.finance.loan.balance_cents;
        if owed == 0 {
            return ActionResult::error("The park owes nothing");
        }
        let cents = match super::pricing::validate_price(cents, content::MAX_CENTS, "A repayment") {
            Ok(0) => return ActionResult::error("Repay more than $0"),
            Ok(cents) => cents,
            Err(message) => return ActionResult::error(message),
        };
        if cents > owed {
            return ActionResult::error(format!("The park owes only {}", dollars(owed)));
        }
        if cents > self.cash_cents {
            return ActionResult::error(format!(
                "Not enough cash: {} on hand",
                dollars(self.cash_cents)
            ));
        }
        let mut ledger = self.finance_today;
        if !ledger.record_financing(true, cents) {
            return ActionResult::error("Repayment exceeds the exact financial range");
        }
        let loan = &mut self.finance.loan;
        loan.balance_cents -= cents;
        loan.repaid_total_cents = loan.repaid_total_cents.saturating_add(cents);
        self.cash_cents -= cents;
        self.finance_today = ledger;
        self.emit("loan_repaid", None);
        ActionResult::ok(format!(
            "Repaid {}; the park owes {}",
            dollars(cents),
            dollars(self.finance.loan.balance_cents)
        ))
    }

    /// Closes the day at midnight: charges the day's interest, files the day in the history
    /// and counts a deficit day. Runs exactly once per day boundary.
    pub(crate) fn close_finance_day(&mut self) {
        let loan = self.finance.loan;
        let interest = daily_interest_cents(loan.peak_cents, loan.rate());
        if interest > 0 {
            let cash = self
                .cash_cents
                .saturating_sub(interest)
                .max(-content::MAX_CENTS);
            let mut ledger = self.finance_today;
            if ledger.record_expense(ExpenseCategory::LoanInterest, interest) {
                self.cash_cents = cash;
                self.finance_today = ledger;
                let loan = &mut self.finance.loan;
                loan.interest_total_cents = loan.interest_total_cents.saturating_add(interest);
            }
        }
        let finance = &mut self.finance;
        finance.loan.peak_cents = finance.loan.balance_cents;
        if finance.history.len() == HISTORY_DAYS {
            finance.history.pop_front();
        }
        finance.history.push_back(DayRecord {
            day: self.day,
            opening_cash_cents: finance.opening_cash_cents,
            closing_cash_cents: self.cash_cents,
            loan_balance_cents: finance.loan.balance_cents,
            ledger: self.finance_today,
        });
        if self.cash_cents < 0 {
            finance.deficit_days = finance.deficit_days.saturating_add(1);
            finance.deficit_streak_days = finance.deficit_streak_days.saturating_add(1);
        } else {
            finance.deficit_streak_days = 0;
        }
        finance.opening_cash_cents = self.cash_cents;
        self.finance_today = FinanceLedger::default();
    }

    /// A day of recurring charges at what the park has now, by ledger line.
    fn commitments(&self) -> Vec<Line> {
        let (charges, _) = self.recurring_hourly_charges();
        charges
            .iter()
            .filter(|(_, cents)| *cents > 0)
            .map(|(category, cents)| Line {
                line: category.ledger_line(),
                cents: cents.saturating_mul(24),
            })
            .collect()
    }

    fn committed_per_day_cents(&self) -> i64 {
        let loan = self.finance.loan;
        self.commitments()
            .iter()
            .fold(0_i64, |total, line| total.saturating_add(line.cents))
            .saturating_add(daily_interest_cents(loan.balance_cents, loan.rate()))
    }

    pub(crate) fn cash_status(&self) -> CashStatus {
        if self.cash_cents < 0 {
            CashStatus::Deficit
        } else if self.cash_cents < self.committed_per_day_cents() {
            CashStatus::Low
        } else {
            CashStatus::Healthy
        }
    }

    /// The last up to `days` completed days, oldest first.
    fn recent_days(&self, days: usize) -> impl Iterator<Item = &DayRecord> {
        let history = &self.finance.history;
        history.iter().skip(history.len().saturating_sub(days))
    }

    /// The income and supplies a day is expected to bring, from recent completed days, and how
    /// many days that averages.
    fn expected_day(&self) -> (usize, i64, i64) {
        let days: Vec<&DayRecord> = self.recent_days(BASIS_DAYS).collect();
        let Ok(count) = i64::try_from(days.len()) else {
            return (0, 0, 0);
        };
        if count == 0 {
            return (0, 0, 0);
        }
        let (income, supplies) = days.iter().fold((0_i64, 0_i64), |(income, supplies), day| {
            (
                income.saturating_add(day.ledger.income_total_cents()),
                supplies.saturating_add(day.ledger.supplies_cents()),
            )
        });
        (days.len(), income / count, supplies / count)
    }

    fn projected_net_per_day(&self, committed: i64) -> i64 {
        let (_, income, supplies) = self.expected_day();
        income.saturating_sub(supplies).saturating_sub(committed)
    }

    fn budget_view(&self) -> BudgetView {
        let commitments = self.commitments();
        let committed = self.committed_per_day_cents();
        let (basis_days, income, supplies) = self.expected_day();
        let projected = income.saturating_sub(supplies).saturating_sub(committed);
        let recent = self
            .recent_days(BASIS_DAYS)
            .fold(self.finance_today, |total, day| total.plus(day.ledger));
        let mut drivers: Vec<Line> = recent
            .expense_lines()
            .iter()
            .filter(|(_, _, cents)| *cents > 0)
            .map(|(line, _, cents)| Line {
                line,
                cents: *cents,
            })
            .collect();
        drivers.sort_by(|a, b| b.cents.cmp(&a.cents).then(a.line.cmp(b.line)));
        drivers.truncate(DRIVERS);
        let mut assumptions = vec![
            "Recurring costs are today's staff, habitats, stands and running plants, charged \
             every hour for 24 hours, plus a day's interest at the current loan balance.",
        ];
        assumptions.push(if basis_days == 0 {
            "No day has closed yet, so income and supplies are not projected."
        } else {
            "Income and supplies (feed, care, stock, repairs, medicine) average the last \
             completed days; guests and sales will vary."
        });
        assumptions.push(
            "Construction, land, animals, hiring and marketing are one-off decisions and are \
             not projected.",
        );
        BudgetView {
            commitments,
            committed_per_day_cents: committed,
            basis_days,
            expected_income_per_day_cents: income,
            expected_supplies_per_day_cents: supplies,
            projected_net_per_day_cents: projected,
            runway_days: runway(self.cash_cents, projected),
            drivers,
            assumptions,
        }
    }

    /// The candidate plan's effect on cash, debt and runway. Changes nothing.
    pub(crate) fn evaluate_budget(&self, spend: f64, borrow: f64) -> Result<BudgetPlan, String> {
        let spend =
            super::pricing::validate_price(spend, content::MAX_CENTS / 2, "Planned spending")?;
        let borrow =
            super::pricing::validate_price(borrow, content::MAX_CENTS / 2, "Planned borrowing")?;
        let loan = self.finance.loan;
        let within_limit = borrow == 0 || (loan.terms.is_some() && borrow <= loan.headroom_cents());
        let balance_after = loan.balance_cents.saturating_add(borrow);
        let cash_after = self.cash_cents.saturating_add(borrow).saturating_sub(spend);
        let interest_after = daily_interest_cents(balance_after, loan.rate());
        let committed = self
            .committed_per_day_cents()
            .saturating_sub(daily_interest_cents(loan.balance_cents, loan.rate()))
            .saturating_add(interest_after);
        let projected = self.projected_net_per_day(committed);
        Ok(BudgetPlan {
            spend_cents: spend,
            borrow_cents: borrow,
            cash_after_cents: cash_after,
            affordable: cash_after >= 0,
            within_limit,
            loan_balance_after_cents: balance_after,
            interest_per_day_after_cents: interest_after,
            projected_net_per_day_cents: projected,
            runway_days: runway(cash_after, projected),
        })
    }

    fn recovery_steps(&self) -> Vec<RecoveryStep> {
        let mut steps = Vec::new();
        let field = self.walk_distance_field(super::Mover::Guest, self.entrance);
        let (fair, _) = self.experience_value(&field, &self.viewing_occupancy());
        if self.pricing.admission_cents < fair {
            steps.push(RecoveryStep::RaiseAdmission {
                admission_cents: self.pricing.admission_cents,
                fair_cents: fair,
            });
        }
        let wage = self.catalog.balance.staff.vendor.hourly_wage_cents;
        for stand in &self.concessions {
            let Some(ops) = stand.stand.as_ref() else {
                continue;
            };
            let margin = ops.today.contribution_cents();
            if ops.vendors > 0 && margin < 0 {
                steps.push(RecoveryStep::CutVendors {
                    stand_id: stand.id,
                    vendors: ops.vendors,
                    wages_per_day_cents: i64::from(ops.vendors)
                        .saturating_mul(wage)
                        .saturating_mul(24),
                    margin_today_cents: margin,
                });
            }
        }
        let invested = self
            .recent_days(BASIS_DAYS)
            .fold(self.finance_today, |total, day| total.plus(day.ledger))
            .investment_cents();
        if invested > 0 {
            steps.push(RecoveryStep::PostponeExpansion {
                invested_cents: invested,
            });
        }
        let animal_upkeep = self.habitats.iter().fold(0_i64, |total, habitat| {
            total.saturating_add(habitat.animal_upkeep_cents())
        });
        if animal_upkeep > 0 {
            steps.push(RecoveryStep::RehomeAnimals {
                animal_upkeep_per_day_cents: animal_upkeep.saturating_mul(24),
            });
        }
        let headroom = self.finance.loan.headroom_cents();
        if headroom > 0 {
            steps.push(RecoveryStep::Borrow {
                headroom_cents: headroom,
            });
        }
        steps
    }

    pub(crate) fn finance_view(&self) -> FinanceView {
        let loan = self.finance.loan;
        let today = FinanceDayView {
            day: self.day,
            opening_cash_cents: self.finance.opening_cash_cents,
            closing_cash_cents: self.cash_cents,
            loan_balance_cents: loan.balance_cents,
            flows: Flows::new(self.finance_today),
        };
        let previous_day = self.finance.history.back().map(FinanceDayView::of);
        let profit_change_cents =
            previous_day.map(|previous| today.flows.profit_cents - previous.flows.profit_cents);
        let profit_trend = match profit_change_cents {
            None => "no_previous_day",
            Some(change) if change > 0 => "up",
            Some(change) if change < 0 => "down",
            Some(_) => "flat",
        };
        let periods = PERIODS
            .iter()
            .map(|&period| {
                let days: Vec<&DayRecord> = self.recent_days(period).collect();
                let ledger = days.iter().fold(FinanceLedger::default(), |total, day| {
                    total.plus(day.ledger)
                });
                PeriodView {
                    period_days: period,
                    days: days.len(),
                    first_day: days.first().map(|day| day.day),
                    last_day: days.last().map(|day| day.day),
                    opening_cash_cents: days.first().map(|day| day.opening_cash_cents),
                    closing_cash_cents: days.last().map(|day| day.closing_cash_cents),
                    flows: Flows::new(ledger),
                }
            })
            .collect();
        let status = self.cash_status();
        FinanceView {
            admission_price_cents: self.pricing.admission_cents,
            current_day: today,
            previous_day,
            profit_change_cents,
            profit_trend,
            solvency: SolvencyView {
                status,
                low_cash_line_cents: self.committed_per_day_cents(),
                deficit_cents: self.cash_cents.min(0).saturating_neg(),
                deficit_days: self.finance.deficit_days,
                deficit_streak_days: self.finance.deficit_streak_days,
                spending_blocked: status == CashStatus::Deficit,
                recovery: if status == CashStatus::Healthy {
                    Vec::new()
                } else {
                    self.recovery_steps()
                },
            },
            loan: LoanView {
                available: loan.terms.is_some(),
                limit_cents: loan.terms.map_or(0, |terms| terms.limit_cents),
                daily_interest_basis_points: loan.rate(),
                balance_cents: loan.balance_cents,
                headroom_cents: loan.headroom_cents(),
                interest_tonight_cents: daily_interest_cents(loan.peak_cents, loan.rate()),
                interest_per_day_cents: daily_interest_cents(loan.balance_cents, loan.rate()),
                borrowed_total_cents: loan.borrowed_total_cents,
                repaid_total_cents: loan.repaid_total_cents,
                interest_total_cents: loan.interest_total_cents,
            },
            budget: self.budget_view(),
            periods,
            history: self
                .finance
                .history
                .iter()
                .rev()
                .map(FinanceDayView::of)
                .collect(),
        }
    }

    /// The low-cash warning and the deficit alert, with what recovers from them.
    pub(crate) fn detect_finance_alerts(
        &self,
        raise: &mut impl FnMut(AlertCode, Severity, Subject, Position, String, AlertAction),
    ) {
        let committed = self.committed_per_day_cents();
        let headroom = self.finance.loan.headroom_cents();
        let bridge = if headroom > 0 {
            format!(
                " Up to {} can still be borrowed as a bridge.",
                dollars(headroom)
            )
        } else {
            String::new()
        };
        match self.cash_status() {
            CashStatus::Healthy => {}
            CashStatus::Low => raise(
                AlertCode::LowCash,
                Severity::Warning,
                Subject::Finances,
                self.entrance,
                format!(
                    "Cash of {} covers less than a day of recurring costs ({} a day). Check \
                     prices and vendors, or postpone building.{bridge}",
                    dollars(self.cash_cents),
                    dollars(committed)
                ),
                AlertAction::OpenFinances,
            ),
            CashStatus::Deficit => raise(
                AlertCode::CashDeficit,
                Severity::Critical,
                Subject::Finances,
                self.entrance,
                format!(
                    "The park is {} in the red: building, hiring and purchases wait until cash \
                     is positive, while wages, upkeep and interest keep accruing. Recover with \
                     fair prices, fewer vendors, closed stands or rehomed animals.{bridge}",
                    dollars(self.cash_cents.saturating_neg())
                ),
                AlertAction::OpenFinances,
            ),
        }
    }

    /// Why the saved finances are inconsistent, if they are.
    pub(crate) fn finance_violation(&self) -> Option<String> {
        let finance = &self.finance;
        let loan = finance.loan;
        let limit = loan.terms.map_or(0, |terms| terms.limit_cents);
        if let Some(terms) = loan.terms
            && (!(1..=content::MAX_CENTS / 2).contains(&terms.limit_cents)
                || terms.daily_interest_basis_points > MAX_DAILY_INTEREST_BASIS_POINTS)
        {
            return Some("loan terms are out of range".to_owned());
        }
        if !(0..=limit).contains(&loan.balance_cents)
            || !(loan.balance_cents..=limit).contains(&loan.peak_cents)
        {
            return Some(format!(
                "loan balance {} (peak {}) is outside 0..={limit}",
                loan.balance_cents, loan.peak_cents
            ));
        }
        if [
            loan.borrowed_total_cents,
            loan.repaid_total_cents,
            loan.interest_total_cents,
        ]
        .iter()
        .any(|total| *total < 0)
        {
            return Some("loan totals are negative".to_owned());
        }
        if finance.history.len() > HISTORY_DAYS {
            return Some(format!(
                "finance history holds {} days, more than {HISTORY_DAYS}",
                finance.history.len()
            ));
        }
        if finance
            .history
            .iter()
            .zip(finance.history.iter().skip(1))
            .any(|(earlier, later)| later.day <= earlier.day || later.day >= self.day)
            || finance
                .history
                .back()
                .is_some_and(|last| last.day >= self.day)
        {
            return Some("finance history days are out of order".to_owned());
        }
        if finance.deficit_streak_days > finance.deficit_days {
            return Some("deficit streak exceeds the deficit days".to_owned());
        }
        None
    }
}

impl ExpenseCategory {
    /// The ledger field this category is recorded in.
    fn ledger_line(self) -> &'static str {
        match self {
            Self::Construction => "construction_expense_cents",
            Self::AnimalPurchase => "animal_purchase_expense_cents",
            Self::HabitatCare => "habitat_care_expense_cents",
            Self::AnimalFeed => "animal_feed_expense_cents",
            Self::KeeperHiring => "keeper_hiring_expense_cents",
            Self::JanitorHiring => "janitor_hiring_expense_cents",
            Self::MechanicHiring => "mechanic_hiring_expense_cents",
            Self::MaintenanceRepair => "maintenance_repair_expense_cents",
            Self::ParkUpkeep => "park_upkeep_expense_cents",
            Self::KeeperWages => "keeper_wages_expense_cents",
            Self::JanitorWages => "janitor_wages_expense_cents",
            Self::MechanicWages => "mechanic_wages_expense_cents",
            Self::Land => "land_expense_cents",
            Self::VendorWages => "vendor_wages_expense_cents",
            Self::StandStock => "stand_stock_expense_cents",
            Self::Marketing => "marketing_expense_cents",
            Self::VetHiring => "vet_hiring_expense_cents",
            Self::VetWages => "vet_wages_expense_cents",
            Self::Medicine => "medicine_expense_cents",
            Self::UtilityOperation => "utility_operation_expense_cents",
            Self::UtilityReplacement => "utility_replacement_expense_cents",
            Self::LoanInterest => "loan_interest_expense_cents",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::daily_interest_cents;

    #[test]
    fn interest_rounds_up_to_a_whole_cent() {
        assert_eq!(daily_interest_cents(0, 20), 0);
        assert_eq!(daily_interest_cents(1, 20), 1);
        assert_eq!(daily_interest_cents(1_000_000, 20), 2_000);
        assert_eq!(daily_interest_cents(1_000_001, 20), 2_001);
        assert_eq!(daily_interest_cents(500_000, 0), 0);
        assert_eq!(daily_interest_cents(-5, 20), 0);
    }
}
