import type { LostDemand, StandOperations } from "./game-types"
import { money } from "./ManagementWorkspace"
import { PriceInput, price } from "./PricingPanel"

const LOST_REASONS: [keyof LostDemand, string][] = [
  ["queue_full", "queue full"],
  ["gave_up", "gave up waiting"],
  ["unaffordable", "short of money"],
  ["too_expensive", "too dear"],
  ["closed", "closed"],
  ["unstaffed", "no vendor"],
  ["out_of_stock", "sold out"],
  ["broken_down", "broken down"],
]

function lostBreakdown(lost: LostDemand) {
  return LOST_REASONS.filter(([key]) => lost[key] > 0)
    .map(([key, label]) => `${lost[key]} ${label}`)
    .join(" · ")
}

function restockNote(stand: StandOperations) {
  if (stand.delivery_in_minutes !== null) {
    return `delivery in ${stand.delivery_in_minutes} min`
  }
  return stand.auto_restock ? `reorders at ${stand.restock_at}` : "automatic restocking off"
}

export type StandCommands = {
  setVendors: (vendors: number) => void
  setOpen: (open: boolean) => void
  restock: () => void
  setAutoRestock: (auto: boolean) => void
  setPrice: (cents: number) => void
}

/**
 * Inspector rows and controls for a food or drink stand (#102). Every value is zoo-core's
 * snapshot; every control sends one zoo-core command.
 */
export function StandRows({
  stand,
  commands,
}: {
  stand: StandOperations
  commands: StandCommands
}) {
  const today = stand.today
  const takesGuests = stand.status === "open" || stand.status === "restocking"
  return (
    <>
      <div className={takesGuests ? "" : "zone-warning"}>
        <dt>Service</dt>
        <dd data-stand-status={stand.status}>{stand.status_label}</dd>
      </div>
      <div>
        <dt>Price</dt>
        <dd data-stand-price={stand.price_cents}>
          <PriceInput
            label="Stand price"
            name="stand-price"
            cents={stand.price_cents}
            onCommit={commands.setPrice}
          />
          <small>
            Usually {price(stand.reference_price_cents)} · {stand.today.lost.too_expensive} found it
            too dear today
          </small>
        </dd>
      </div>
      <div className={stand.vendors === 0 ? "zone-warning" : ""}>
        <dt>Vendors</dt>
        <dd className="stand-stepper">
          <button
            type="button"
            aria-label="Remove a vendor"
            disabled={stand.vendors === 0}
            onClick={() => commands.setVendors(stand.vendors - 1)}
          >
            −
          </button>
          <span data-stand-vendors={stand.vendors}>
            {stand.vendors} of {stand.max_vendors} · {money(stand.vendor_hourly_wage_cents)}/h each
          </span>
          <button
            type="button"
            aria-label="Add a vendor"
            disabled={stand.vendors >= stand.max_vendors}
            onClick={() => commands.setVendors(stand.vendors + 1)}
          >
            +
          </button>
        </dd>
      </div>
      <div className={stand.queue >= stand.queue_capacity ? "zone-warning" : ""}>
        <dt>Queue</dt>
        <dd data-stand-queue={stand.queue}>
          {stand.serving} served now · {stand.queue}/{stand.queue_capacity} waiting
          {stand.heading > 0 ? ` · ${stand.heading} on the way` : ""}
        </dd>
      </div>
      <div>
        <dt>Speed</dt>
        <dd data-stand-capacity={stand.capacity_per_hour}>
          {stand.service_minutes} min a sale · up to {stand.capacity_per_hour} an hour
        </dd>
      </div>
      <div className={stand.stock === 0 ? "zone-warning" : ""}>
        <dt>Stock</dt>
        <dd data-stand-stock={stand.stock}>
          {stand.stock}/{stand.stock_capacity} · {restockNote(stand)}
        </dd>
      </div>
      <div>
        <dt>Sales today</dt>
        <dd data-stand-sales={today.served}>
          {today.served} · {money(today.revenue_cents)}
          {today.average_wait_minutes !== null
            ? ` · ${today.average_wait_minutes} min average wait`
            : ""}
        </dd>
      </div>
      <div className={today.lost_sales > 0 ? "zone-warning" : ""}>
        <dt>Lost today</dt>
        <dd data-stand-lost={today.lost_sales}>
          {today.lost_sales === 0 ? "None" : `${today.lost_sales}: ${lostBreakdown(today.lost)}`}
        </dd>
      </div>
      <div>
        <dt>Costs today</dt>
        <dd>
          {money(today.operating_cents)} (stock {money(today.stock_cost_cents)} · wages{" "}
          {money(today.wage_cents)} · upkeep {money(today.upkeep_cents)}
          {today.repair_cents > 0 ? ` · repairs ${money(today.repair_cents)}` : ""})
        </dd>
      </div>
      <div className={today.contribution_cents < 0 ? "zone-warning" : ""}>
        <dt>Margin today</dt>
        <dd data-stand-margin={today.contribution_cents}>
          {money(today.contribution_cents)} · {money(stand.total.contribution_cents)} all time
        </dd>
      </div>
    </>
  )
}

/** Opening, restocking and the resupply policy, below the stand's rows. */
export function StandActions({
  stand,
  commands,
}: {
  stand: StandOperations
  commands: StandCommands
}) {
  return (
    <div className="stand-actions">
      <button type="button" onClick={() => commands.setOpen(!stand.open)}>
        {stand.open ? "Close stand" : "Open stand"}
      </button>
      <button
        type="button"
        disabled={stand.delivery_in_minutes !== null || stand.stock >= stand.stock_capacity}
        onClick={commands.restock}
      >
        Restock · {money(stand.refill_cost_cents)}
      </button>
      <label>
        <input
          type="checkbox"
          checked={stand.auto_restock}
          onChange={(event) => commands.setAutoRestock(event.target.checked)}
        />
        Auto restock
      </label>
    </div>
  )
}
