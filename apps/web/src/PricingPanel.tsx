// The pricing panel (#106): admission and stand prices, the park gate, observed demand and
// marketing. Every figure, forecast and validation comes from zoo-core; this file keeps only
// the player's unfinished price drafts and lays the report out.
import { useId, useState, type KeyboardEvent } from "react"

import type { AdmissionEvaluation, GateNote, PricingState, VisitorProfile } from "./game-types"
import { money } from "./ManagementWorkspace"

export type PricingCommands = {
  setAdmission: (cents: number) => void
  setParkOpen: (open: boolean) => void
  setStandPrice: (standId: number, cents: number) => void
  startCampaign: () => void
  /** zoo-core's forecast for a candidate price; changes nothing. */
  evaluateAdmission: (cents: number) => AdmissionEvaluation | null
}

/** A price with its cents, e.g. "$12" or "$3.50". */
export function price(cents: number) {
  const whole = Math.trunc(cents / 100)
  const rest = Math.abs(cents % 100)
  const sign = cents < 0 && whole === 0 ? "-" : ""
  return rest === 0 ? `${sign}$${whole}` : `${sign}$${whole}.${String(rest).padStart(2, "0")}`
}

/** Exact dollars-and-cents text ("12", "12.5", "$12.50") as whole cents; `null` otherwise. */
export function parseCents(text: string): number | null {
  const match = /^\s*\$?\s*(\d{1,7})(?:\.(\d{1,2}))?\s*$/.exec(text)
  if (!match) {
    return null
  }
  const dollars = Number(match[1])
  const cents = match[2] === undefined ? 0 : Number(match[2].padEnd(2, "0"))
  return dollars * 100 + cents
}

/**
 * Exact price entry: the field shows the authoritative price until the player edits it, keeps
 * an unfinished draft without committing it, commits on Enter or Set and reverts on Escape.
 * zoo-core validates the range and refuses anything else.
 */
export function PriceInput({
  label,
  cents,
  onCommit,
  onDraft,
  name,
}: {
  label: string
  cents: number
  onCommit: (cents: number) => void
  onDraft?: (cents: number | null) => void
  name: string
}) {
  const id = useId()
  const [draft, setDraft] = useState<string | null>(null)
  const text = draft ?? (cents / 100).toFixed(2)
  const parsed = draft === null ? cents : parseCents(draft)
  const invalid = draft !== null && parsed === null
  const update = (next: string | null) => {
    setDraft(next)
    onDraft?.(next === null ? null : parseCents(next))
  }
  const commit = () => {
    if (parsed === null) {
      return
    }
    update(null)
    if (parsed !== cents) {
      onCommit(parsed)
    }
  }
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault()
      commit()
    } else if (event.key === "Escape") {
      event.preventDefault()
      update(null)
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
          aria-invalid={invalid}
          aria-describedby={invalid ? `${id}-error` : undefined}
          data-price-input={name}
          onChange={(event) => update(event.target.value)}
          onKeyDown={onKeyDown}
        />
        <button
          type="button"
          aria-label={`Set ${label.toLowerCase()}`}
          disabled={draft === null || invalid}
          onClick={commit}
        >
          Set
        </button>
      </span>
      {invalid && (
        <small id={`${id}-error`} className="price-error" role="alert">
          Enter dollars and cents, like 12.50
        </small>
      )}
    </div>
  )
}

const PROFILE_LABELS: Record<VisitorProfile, string> = {
  family: "Family",
  enthusiast: "Enthusiasts",
  stroller: "Strollers",
}

function gateLine(note: GateNote) {
  const who = `${PROFILE_LABELS[note.profile]} of ${note.size}`
  switch (note.outcome) {
    case "admitted":
      return `${who} paid ${price(note.price_cents)} each`
    case "closed":
      return `${who} found the park closed`
    case "too_expensive":
      return `${who}: ${price(note.price_cents)} too dear (would pay ${price(note.willing_cents)})`
    case "over_budget":
      return `${who}: ${price(note.price_cents)} over budget (could pay ${price(note.willing_cents)})`
  }
}

/** zoo-core's admission forecast for the current or typed price, or why it cannot be made. */
function ForecastView({
  evaluation,
  cents,
}: {
  evaluation: AdmissionEvaluation | null
  cents: number
}) {
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
  const forecast = evaluation.forecast
  return (
    <div
      className="pricing-forecast"
      data-forecast-price={cents}
      data-forecast-accepting={forecast.accepting_people}
      data-forecast-sample={forecast.sample_people}
    >
      <strong>
        {forecast.sample_groups === 0
          ? `Forecast at ${price(cents)}: no groups yet`
          : `At ${price(cents)}: ${forecast.accepting_people} of ${forecast.sample_people} recent visitors would pay · ${money(forecast.admission_cents)}`}
      </strong>
      <small>{forecast.assumptions}</small>
    </div>
  )
}
export function PricingPanel({
  pricing,
  commands,
}: {
  pricing: PricingState
  commands: PricingCommands
}) {
  const [candidate, setCandidate] = useState<number | null>(null)
  const forecastPrice = candidate ?? pricing.admission_price_cents
  const evaluation =
    candidate === null
      ? ({ ok: true, forecast: pricing.forecast } as const)
      : commands.evaluateAdmission(candidate)
  const today = pricing.today
  return (
    <div
      className="pricing-panel"
      data-park-open={pricing.open}
      data-admission-price={pricing.admission_price_cents}
    >
      <section aria-label="Park gate">
        <div className="pricing-gate">
          <PriceInput
            label="Admission per person"
            name="admission"
            cents={pricing.admission_price_cents}
            onCommit={commands.setAdmission}
            onDraft={setCandidate}
          />
          <button
            type="button"
            className={pricing.open ? "" : "primary"}
            onClick={() => commands.setParkOpen(!pricing.open)}
          >
            {pricing.open ? "Close park" : "Open park"}
          </button>
        </div>
        <p className="pricing-note" data-experience-value={pricing.experience_value_cents}>
          Guests value what they can reach at {price(pricing.experience_value_cents)} a person (
          {pricing.valued_habitats} {pricing.valued_habitats === 1 ? "habitat" : "habitats"}).
          {pricing.open ? "" : " The park is closed: arriving groups are turned away."}
        </p>
        <ForecastView evaluation={evaluation} cents={forecastPrice} />
        <dl
          className="pricing-figures"
          data-gate-arrivals={today.people_at_gate}
          data-gate-admitted={today.people_admitted}
          data-gate-turned-away={pricing.turned_away_today}
          data-gate-too-expensive={today.turned_away_too_expensive}
          data-gate-admission={today.admission_cents}
          data-price-complaints={pricing.price_complaints_today}
        >
          <dt>At the gate today</dt>
          <dd>
            {today.people_at_gate} people in {today.groups_at_gate} groups
          </dd>
          <dt>Admitted</dt>
          <dd>
            {today.people_admitted} · {money(today.admission_cents)}
          </dd>
          <dt className={pricing.turned_away_today > 0 ? "zone-warning" : ""}>Turned away</dt>
          <dd>
            {pricing.turned_away_today === 0
              ? "Nobody"
              : `${pricing.turned_away_today}: ${today.turned_away_too_expensive} too dear · ${today.turned_away_over_budget} over budget · ${today.turned_away_closed} closed`}
          </dd>
          <dt>Price complaints</dt>
          <dd>{pricing.price_complaints_today}</dd>
          <dt>Left for poor value</dt>
          <dd>{pricing.poor_value_exits_today}</dd>
          <dt>Guests' sense of value</dt>
          <dd>{pricing.average_value === null ? "No guests" : `${pricing.average_value}/100`}</dd>
          <dt>Income · profit today</dt>
          <dd className={pricing.profit_today_cents < 0 ? "zone-warning" : ""}>
            {money(pricing.income_today_cents)} · {money(pricing.profit_today_cents)}
          </dd>
        </dl>
        {pricing.recent.length > 0 && (
          <ul className="drill-list gate-recent" aria-label="Latest groups at the gate">
            {pricing.recent.map((note) => (
              <li key={note.arrival} data-gate-outcome={note.outcome}>
                {gateLine(note)}
              </li>
            ))}
          </ul>
        )}
      </section>
      <section aria-label="Stand prices">
        <h3>Stands</h3>
        {pricing.stands.length === 0 ? (
          <p className="empty-state">No food or drink stands yet.</p>
        ) : (
          <ul className="stand-prices">
            {pricing.stands.map((stand) => (
              <li
                key={stand.id}
                data-stand-id={stand.id}
                data-stand-price={stand.price_cents}
                data-stand-sold={stand.served_today}
                data-stand-too-expensive={stand.too_expensive_today}
              >
                <PriceInput
                  label={stand.label}
                  name={`stand-${stand.id}`}
                  cents={stand.price_cents}
                  onCommit={(cents) => commands.setStandPrice(stand.id, cents)}
                />
                <small>
                  Usual {price(stand.reference_price_cents)} · {stand.served_today} sold ·{" "}
                  {stand.too_expensive_today} too dear
                  {stand.unaffordable_today > 0
                    ? ` · ${stand.unaffordable_today} short of money`
                    : ""}{" "}
                  · margin {money(stand.contribution_today_cents)}
                </small>
              </li>
            ))}
          </ul>
        )}
      </section>
      <section aria-label="Marketing" className="pricing-marketing">
        <button
          type="button"
          disabled={pricing.campaign !== null}
          data-campaign-running={pricing.campaign !== null}
          onClick={commands.startCampaign}
        >
          Run campaign · {money(pricing.campaign_cost_cents)}
        </button>
        <small>
          {pricing.campaign === null
            ? `Groups come ${pricing.campaign_arrival_boost_percent}% more often for ${pricing.campaign_minutes / 60} h; prices still decide who enters.`
            : `Running: ${Math.ceil(pricing.campaign.ends_in_minutes / 60)} h left.`}
        </small>
      </section>
    </div>
  )
}
