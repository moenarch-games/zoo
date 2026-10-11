// Independent supplemental boundary coverage after economic repair 7988b30.
use super::*;

fn snapshot(state: &GameState) -> String {
    format!("{state:?}")
}
fn attraction() -> GameState {
    let mut state = GameState::default();
    assert!(state.place_habitat(3, 8, HabitatOrientation::Horizontal).ok);
    staff_habitat(&mut state, 1);
    assert!(state.adopt(1, "capybara").ok);
    state
}

#[test]
fn exact_cash_and_daily_aggregate_edges_commit_once_or_preserve_state() {
    assert_eq!(content::MAX_CENTS, 9_007_199_254_740_991);
    let mut state = GameState::default();
    state.cash_cents = content::MAX_CENTS - 1;
    assert!(state.earn(1, IncomeCategory::Admissions));
    assert_eq!(state.cash_cents, content::MAX_CENTS);
    let before = snapshot(&state);
    assert!(!state.earn(1, IncomeCategory::Admissions));
    assert_eq!(snapshot(&state), before);
    state.cash_cents = 100;
    state.finance_today = FinanceLedger::default();
    state.finance_today.admissions_income_cents = content::MAX_CENTS / 2 - 1;
    assert!(state.earn(1, IncomeCategory::Concessions));
    let before = snapshot(&state);
    assert!(!state.earn(1, IncomeCategory::Admissions));
    assert_eq!(snapshot(&state), before);
    state.finance_today = FinanceLedger::default();
    state.finance_today.construction_expense_cents = content::MAX_CENTS / 2 - 1;
    assert!(state.spend(1, ExpenseCategory::AnimalPurchase).is_ok());
    let before = snapshot(&state);
    assert!(state.spend(1, ExpenseCategory::Construction).is_err());
    assert_eq!(snapshot(&state), before);
    assert!(state.spend(-1, ExpenseCategory::Construction).is_err());
    assert_eq!(snapshot(&state), before);
}

#[test]
fn upkeep_failure_after_first_category_does_not_partially_charge_any_category() {
    let mut state = attraction();
    assert!(state.hire_janitor().ok);
    assert!(state.hire_mechanic().ok);
    let mut normal = state.clone();
    let old_cash = normal.cash_cents;
    normal.charge_upkeep();
    let full_charge = old_cash - normal.cash_cents;
    assert!(full_charge > 0);
    assert!(normal.finance_today.park_upkeep_expense_cents > 0);
    assert!(normal.finance_today.keeper_wages_expense_cents > 0);
    assert!(normal.finance_today.janitor_wages_expense_cents > 0);
    assert!(normal.finance_today.mechanic_wages_expense_cents > 0);
    state.finance_today = FinanceLedger::default();
    state.finance_today.construction_expense_cents = content::MAX_CENTS / 2 - full_charge + 1;
    let before = snapshot(&state);
    state.charge_upkeep();
    assert_eq!(snapshot(&state), before);
    state.finance_today = FinanceLedger::default();
    state.cash_cents = -content::MAX_CENTS + full_charge - 1;
    let before = snapshot(&state);
    state.charge_upkeep();
    assert_eq!(snapshot(&state), before);
}

#[test]
fn failed_sale_does_not_credit_money_restore_guest_or_create_side_effects() {
    for boundary in 0..5 {
        let mut state = attraction();
        assert!(state.place_concession(1, ENTRANCE_Y - 1, "drink").ok);
        state.try_spawn_guest();
        assert_eq!(state.guests.len(), 1);
        let guest = &mut state.guests[0];
        guest.x = 1;
        guest.y = ENTRANCE_Y;
        guest.state = GuestState::WalkingToHabitat;
        guest.thirst = DRINK_BUY_THRESHOLD;
        match boundary {
            0 => state.concessions[0].total_revenue_cents = content::MAX_CENTS,
            1 => state.concessions[0].sales_today = u32::MAX,
            2 => state.concessions[0].total_sales = u32::MAX,
            3 => state.cash_cents = content::MAX_CENTS,
            _ => state.finance_today.admissions_income_cents = content::MAX_CENTS / 2,
        }
        let mut positive = state.clone();
        positive.cash_cents = 100_000;
        positive.finance_today = FinanceLedger::default();
        positive.concessions[0].total_revenue_cents = 0;
        positive.concessions[0].sales_today = 0;
        positive.concessions[0].total_sales = 0;
        positive.serve_concessions();
        assert!(positive.guests[0].bought_drink);
        assert_eq!(positive.concessions[0].total_sales, 1);
        let before = snapshot(&state);
        state.serve_concessions();
        assert_eq!(
            snapshot(&state),
            before,
            "Sale boundary {boundary} mutated park"
        );
        assert!(!state.guests[0].bought_drink);
        assert_eq!(state.guests[0].thirst, DRINK_BUY_THRESHOLD);
    }
}

#[test]
fn guest_identifier_exhaustion_does_not_charge_admission() {
    let mut state = attraction();
    state.next_guest_id = u32::MAX;
    let before = snapshot(&state);
    state.try_spawn_guest();
    assert_eq!(snapshot(&state), before);
    assert!(state.guests.is_empty());
    assert_eq!(state.next_guest_id, u32::MAX);
}

#[test]
fn two_day_profit_delta_stays_in_exact_javascript_integer_domain() {
    let half = 9_007_199_254_740_991_i64 / 2;
    let income = FinanceLedger {
        admissions_income_cents: half,
        ..Default::default()
    };
    let expense = FinanceLedger {
        construction_expense_cents: half,
        ..Default::default()
    };
    assert_eq!(income.profit_cents() - expense.profit_cents(), half * 2);
    assert_eq!(expense.profit_cents() - income.profit_cents(), -half * 2);
    assert!(half * 2 <= 9_007_199_254_740_991);
}
