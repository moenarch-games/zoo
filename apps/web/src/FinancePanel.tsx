// The finances panel (#115): cash status and recovery, operating versus borrowed cash by day
// and period, the loan with a budget plan, recurring commitments and the kept history. Every
// figure, projection, validation and policy comes from zoo-core; this file keeps only the
// player's unfinished amounts and lays the report out.
import { useId, useState, type KeyboardEvent, type ReactNode } from "react"

import type {
  BudgetEvaluation,
  FinanceBreakdown,
  FinanceDay,
  FinanceFlows,
  FinanceState,
  FinanceVerdict,
  RecoveryStep,
} from "./game-types"
import { LEDGER_LABELS, money, price } from "./ManagementWorkspace"
import { parseCents } from "./PricingPanel"

export type FinanceCommands = {
  borrow: (cents: number) => void
  repay: (cents: number) => void
  setAdmission: (cents: number) => void
  setStandVendors: (standId: number, vendors: number) => void
  /** zoo-core's plan for candidate spending and borrowing; changes nothing. */
  evaluateBudget: (spendCents: number, borrowCents: number) => BudgetEvaluation | null
}

const STATUS_LABELS: Record<FinanceState["solvency"]["status"], string> = {
  healthy: "Cash is healthy",
  low: "Low cash",
  deficit: "In deficit",
}

const VERDICTS: Record<FinanceVerdict, string> = {
  operations_profitable: "Operations are profitable.",
  borrowing_covers_losses: "Operations lose money; borrowed cash is covering it.",
  operations_losing: "Operations lose money; cash on hand is paying for it.",
  no_activity: "No operating activity yet.",
}

function lineLabel(line: keyof FinanceBreakdown) {
  return LEDGER_LABELS[line] ?? line
}

function percentPerDay(basisPoints: number) {
  return `${(basisPoints / 100).toLocaleString(undefined, { maximumFractionDigits: 2 })}% a day`
}

function days(count: number) {
  return `${count} ${count === 1 ? "day" : "days"}`
}

function stepView(
  step: RecoveryStep,
  commands: FinanceCommands,
): { text: string; action?: ReactNode } {
  switch (step.kind) {
    case "raise_admission":
      return {
        text: `Admission is ${price(step.admission_cents)}; guests value the park at ${price(step.fair_cents)}.`,
        action: (
          <button type="button" onClick={() => commands.setAdmission(step.fair_cents)}>
            Charge {price(step.fair_cents)}
          </button>
        ),
      }
    case "cut_vendors":
      return {
        text: `Stand #${step.stand_id} lost ${money(-step.margin_today_cents)} today; its ${step.vendors} ${step.vendors === 1 ? "vendor costs" : "vendors cost"} ${money(step.wages_per_day_cents)} a day.`,
        action: (
          <button type="button" onClick={() => commands.setStandVendors(step.stand_id, 0)}>
            Recall vendors
          </button>
        ),
      }
    case "postpone_expansion":
      return {
        text: `${money(step.invested_cents)} went into building, land and animals this week; postpone more until cash recovers.`,
      }
    case "rehome_animals":
      return {
        text: `Animals cost ${money(step.animal_upkeep_per_day_cents)} a day to keep; rehoming lowers that, with no refund.`,
      }
    case "borrow":
      return { text: `${money(step.headroom_cents)} of credit is left as a bridge.` }
  }
}

function flowRows(flows: FinanceFlows[]) {
  const row = (label: string, pick: (flow: FinanceFlows) => number, key: string) => (
    <tr key={key} data-row={key}>
      <th scope="row">{label}</th>
      {flows.map((flow, index) => (
        <td key={index} className={pick(flow) < 0 ? "zone-warning" : ""}>
          {money(pick(flow))}
        </td>
      ))}
    </tr>
  )
  return [
    row("Operating income", (flow) => flow.income_cents, "income"),
    row("Operating costs", (flow) => -flow.operating_expenses_cents, "operating-costs"),
    row("Operating result", (flow) => flow.operating_result_cents, "operating-result"),
    row("Investment", (flow) => -flow.investment_cents, "investment"),
    row("Interest", (flow) => -flow.interest_cents, "interest"),
    row("Net result", (flow) => flow.profit_cents, "profit"),
    row("Borrowed", (flow) => flow.loan_proceeds_cents, "borrowed"),
    row("Repaid", (flow) => -flow.loan_repayments_cents, "repaid"),
    row("Net cash flow", (flow) => flow.net_cash_flow_cents, "net-cash-flow"),
  ]
}

/** Exact amount entry that starts empty; Enter runs the field's action, Escape clears it. */
function AmountField({
  label,
  name,
  text,
  onText,
  onEnter,
}: {
  label: string
  name: string
  text: string
  onText: (text: string) => void
  onEnter: () => void
}) {
  const id = useId()
  const invalid = text.trim() !== "" && parseCents(text) === null
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault()
      onEnter()
    } else if (event.key === "Escape") {
      event.preventDefault()
      onText("")
    }
  }
  return (
    <div className="price-input">
      <label htmlFor={id}>{label}</label>
      <span className="price-field">
        <span aria-hidden="true">$</span>
        <input
          id={id}
          name={name}
          inputMode="decimal"
          autoComplete="off"
          value={text}
          placeholder="0"
          aria-invalid={invalid}
          data-amount-input={name}
          onChange={(event) => onText(event.target.value)}
          onKeyDown={onKeyDown}
        />
      </span>
      {invalid && (
        <small className="price-error" role="alert">
          Enter dollars and cents, like 1200.50
        </small>
      )}
    </div>
  )
}

function PlanView({ evaluation }: { evaluation: BudgetEvaluation | null }) {
  if (evaluation === null) {
    return null
  }
  if (!evaluation.ok) {
    return (
      <p className="price-error" role="alert">
        {evaluation.message}
      </p>
    )
  }
  const plan = evaluation.plan
  return (
    <div
      className="pricing-forecast budget-plan"
      data-plan-borrow={plan.borrow_cents}
      data-plan-within-limit={plan.within_limit}
      data-plan-affordable={plan.affordable}
      data-plan-interest={plan.interest_per_day_after_cents}
    >
      <strong>
        Cash after: {money(plan.cash_after_cents)} · owed {money(plan.loan_balance_after_cents)} ·
        interest {money(plan.interest_per_day_after_cents)} a day
      </strong>
      <small>
        {plan.within_limit ? "" : "Over the credit limit. "}
        {plan.affordable ? "" : "Not enough cash for that spending. "}
        Projected {money(plan.projected_net_per_day_cents)} a day
        {plan.runway_days === null ? "" : `; cash lasts ${days(plan.runway_days)}`}.
      </small>
    </div>
  )
}

export function FinancePanel({
  finance,
  cash,
  commands,
  ledger,
}: {
  finance: FinanceState
  cash: number
  commands: FinanceCommands
  ledger: ReactNode
}) {
  const [spendText, setSpendText] = useState("")
  const [borrowText, setBorrowText] = useState("")
  const [repayText, setRepayText] = useState("")
  const { solvency, loan, budget } = finance
  const spend = spendText.trim() === "" ? 0 : parseCents(spendText)
  const borrowCents = borrowText.trim() === "" ? 0 : parseCents(borrowText)
  const repayCents = parseCents(repayText)
  const planning = spendText.trim() !== "" || borrowText.trim() !== ""
  const evaluation =
    planning && spend !== null && borrowCents !== null
      ? commands.evaluateBudget(spend, borrowCents)
      : null
  const canBorrow =
    loan.available && borrowCents !== null && borrowCents > 0 && borrowCents <= loan.headroom_cents
  const canRepay =
    repayCents !== null && repayCents > 0 && repayCents <= loan.balance_cents && repayCents <= cash
  const borrow = () => {
    if (canBorrow && borrowCents !== null) {
      setBorrowText("")
      commands.borrow(borrowCents)
    }
  }
  const repay = () => {
    if (canRepay && repayCents !== null) {
      setRepayText("")
      commands.repay(repayCents)
    }
  }
  const [week, month] = finance.periods
  const columns: { label: string; flows: FinanceFlows }[] = [
    { label: `Day ${finance.current_day.day} so far`, flows: finance.current_day },
    ...finance.periods.map((period) => ({
      label: `Last ${period.period_days} days`,
      flows: period.flows,
    })),
  ]
  return (
    <div
      className="finance-panel pricing-panel"
      data-cash={cash}
      data-status={solvency.status}
      data-deficit-days={solvency.deficit_days}
      data-loan-balance={loan.balance_cents}
      data-loan-headroom={loan.headroom_cents}
      data-interest-total={loan.interest_total_cents}
      data-history-days={finance.history.length}
    >
      <section aria-label="Cash" className={`finance-status ${solvency.status}`}>
        <strong>
          {STATUS_LABELS[solvency.status]} · {money(cash)}
          {loan.balance_cents > 0 ? ` · owes ${money(loan.balance_cents)}` : ""}
        </strong>
        <small>
          {solvency.status === "deficit"
            ? `Building, hiring and purchases wait until cash is positive; wages, upkeep and interest keep accruing.`
            : `Recurring costs ${money(budget.committed_per_day_cents)} a day.`}
          {budget.runway_days === null || solvency.status === "deficit"
            ? ""
            : ` Cash lasts about ${days(budget.runway_days)}.`}
          {solvency.deficit_days > 0
            ? ` ${days(solvency.deficit_days)} closed in deficit${solvency.deficit_streak_days > 0 ? `, ${solvency.deficit_streak_days} in a row` : ""}.`
            : ""}
        </small>
        {solvency.recovery.length > 0 && (
          <ul className="drill-list finance-recovery" aria-label="Recovery steps">
            {solvency.recovery.map((step, index) => {
              const { text, action } = stepView(step, commands)
              return (
                <li key={`${step.kind}:${index}`} data-step={step.kind}>
                  <span>{text}</span>
                  {action}
                </li>
              )
            })}
          </ul>
        )}
      </section>
      <section aria-label="Operations and borrowing">
        <table className="finance-report">
          <thead>
            <tr>
              <th scope="col">Flows</th>
              {columns.map((column) => (
                <th key={column.label} scope="col">
                  {column.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>{flowRows(columns.map((column) => column.flows))}</tbody>
        </table>
        {week && (
          <p className="pricing-note finance-verdict" data-verdict={week.flows.verdict}>
            {week.days === 0
              ? "No day has closed yet."
              : `Last ${days(week.days)}: ${VERDICTS[week.flows.verdict]}`}
            {month && month.days > week.days
              ? ` Last ${days(month.days)}: ${VERDICTS[month.flows.verdict]}`
              : ""}
          </p>
        )}
      </section>
      <section aria-label="Loan" className="finance-loan">
        <h3>Loan</h3>
        {loan.available ? (
          <p className="pricing-note">
            Owes {money(loan.balance_cents)} of {money(loan.limit_cents)} at{" "}
            {percentPerDay(loan.daily_interest_basis_points)}, charged at midnight on the day's
            highest balance: {money(loan.interest_tonight_cents)} tonight. Paid so far:{" "}
            {money(loan.interest_total_cents)} interest, {money(loan.repaid_total_cents)} repaid.
          </p>
        ) : (
          <p className="pricing-note">This scenario offers no loans.</p>
        )}
        <div className="pricing-gate">
          <AmountField
            label="Planned spending"
            name="spend"
            text={spendText}
            onText={setSpendText}
            onEnter={() => undefined}
          />
          {loan.available && (
            <>
              <AmountField
                label="Borrow"
                name="borrow"
                text={borrowText}
                onText={setBorrowText}
                onEnter={borrow}
              />
              <button type="button" disabled={!canBorrow} onClick={borrow}>
                Borrow
              </button>
            </>
          )}
        </div>
        <PlanView evaluation={evaluation} />
        {loan.balance_cents > 0 && (
          <div className="pricing-gate">
            <AmountField
              label="Repay"
              name="repay"
              text={repayText}
              onText={setRepayText}
              onEnter={repay}
            />
            <button type="button" disabled={!canRepay} onClick={repay}>
              Repay
            </button>
          </div>
        )}
      </section>
      <section aria-label="Budget">
        <h3>Budget</h3>
        <dl className="pricing-figures" data-projected={budget.projected_net_per_day_cents}>
          {budget.commitments.map((line) => (
            <FinanceFigure key={line.line} label={lineLabel(line.line)} cents={-line.cents} />
          ))}
          {loan.interest_per_day_cents > 0 && (
            <FinanceFigure label="Loan interest" cents={-loan.interest_per_day_cents} />
          )}
          <dt>Committed a day</dt>
          <dd>{money(-budget.committed_per_day_cents)}</dd>
          {budget.basis_days > 0 && (
            <>
              <FinanceFigure label="Expected income" cents={budget.expected_income_per_day_cents} />
              <FinanceFigure
                label="Expected supplies"
                cents={-budget.expected_supplies_per_day_cents}
              />
              <dt>Projected a day</dt>
              <dd className={budget.projected_net_per_day_cents < 0 ? "zone-warning" : ""}>
                {money(budget.projected_net_per_day_cents)}
              </dd>
            </>
          )}
        </dl>
        {budget.drivers.length > 0 && (
          <p className="pricing-note finance-drivers">
            Largest costs this week:{" "}
            {budget.drivers
              .map((line) => `${lineLabel(line.line)} ${money(line.cents)}`)
              .join(" · ")}
          </p>
        )}
        <ul className="finance-assumptions">
          {budget.assumptions.map((text) => (
            <li key={text}>
              <small>{text}</small>
            </li>
          ))}
        </ul>
      </section>
      {finance.history.length > 0 && (
        <section aria-label="Daily history">
          <h3>Recent days</h3>
          <table className="finance-report finance-history">
            <thead>
              <tr>
                <th scope="col">Day</th>
                <th scope="col">Operations</th>
                <th scope="col">Cash flow</th>
                <th scope="col">Closing cash</th>
                <th scope="col">Owed</th>
              </tr>
            </thead>
            <tbody>
              {finance.history.slice(0, 7).map((day: FinanceDay) => (
                <tr key={day.day} data-day={day.day} data-verdict={day.verdict}>
                  <th scope="row">{day.day}</th>
                  <td className={day.operating_result_cents < 0 ? "zone-warning" : ""}>
                    {money(day.operating_result_cents)}
                  </td>
                  <td>{money(day.net_cash_flow_cents)}</td>
                  <td className={day.closing_cash_cents < 0 ? "zone-warning" : ""}>
                    {money(day.closing_cash_cents)}
                  </td>
                  <td>{money(day.loan_balance_cents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
      <section aria-label="Ledger">{ledger}</section>
    </div>
  )
}

function FinanceFigure({ label, cents }: { label: string; cents: number }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{money(cents)}</dd>
    </>
  )
}
