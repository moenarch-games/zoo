// Land and expansion planning (#99): presents zoo-core's parcels, their prices, trade-offs and
// restrictions, the Rust quote for the selected parcels, and the player's non-binding plan of
// prospective paths and habitats. Ownership, prices, quotes and every plan item's validity and
// cost come from Rust; this module only lays the values out and gathers selections.
import type { LandQuote, LandState, Parcel, PathAccess, Point } from "./game-types"
import { money } from "./ManagementWorkspace"

const STATUS_LABEL: Record<Parcel["status"], string> = {
  owned: "Owned",
  available: "For sale",
  unaffordable: "Too expensive",
  locked: "Locked",
}

/** One prospective construction gesture, kept until it is built or removed; never saved. */
export type PlanItem =
  | { id: number; kind: "path"; points: number[]; width: number; access: PathAccess }
  | { id: number; kind: "plaza"; a: Point; b: Point; access: PathAccess }
  | { id: number; kind: "habitat"; a: Point; b: Point }
  | { id: number; kind: "outline"; points: number[] }

/** Rust's evaluation of a plan item against the current park. */
export type PlanEvaluation = {
  item: PlanItem
  ok: boolean
  message: string
  cost_cents: number
  tiles: Point[]
  land_parcels: string[]
}

export function planLabel(item: PlanItem) {
  switch (item.kind) {
    case "path":
      return `${item.access === "staff" ? "Staff-only path" : "Path"} stroke`
    case "plaza":
      return `${item.access === "staff" ? "Staff-only plaza" : "Plaza"}`
    case "habitat":
    case "outline":
      return "Habitat"
  }
}

/** The parcel whose rectangles contain `tile`; picking is presentation, not ownership truth. */
export function parcelAt(land: LandState, tile: Point) {
  return (
    land.parcels.find((parcel) =>
      parcel.areas.some(
        (area) =>
          tile.x >= area.x &&
          tile.x < area.x + area.width &&
          tile.y >= area.y &&
          tile.y < area.y + area.height,
      ),
    ) ?? null
  )
}

export function parcelCenter(parcel: Parcel): Point {
  const area = parcel.areas[0]
  return area
    ? { x: Math.floor(area.x + area.width / 2), y: Math.floor(area.y + area.height / 2) }
    : { x: 0, y: 0 }
}

function checkMark(parcel: Parcel, selected: boolean) {
  if (parcel.owned) {
    return ""
  }
  return selected ? "☑ " : "☐ "
}

function facts(parcel: Parcel) {
  const parts = [`${parcel.tiles} tiles`, `${parcel.buildable_tiles} buildable`]
  if (parcel.water_tiles > 0) {
    parts.push(`${parcel.water_tiles} water`)
  }
  parts.push(parcel.path_frontage ? "path reaches it" : "no path yet")
  if (parcel.price_per_buildable_tile_cents !== null) {
    parts.push(`${money(parcel.price_per_buildable_tile_cents)}/buildable tile`)
  }
  return parts.join(" · ")
}

export function LandPanel({
  land,
  selection,
  quote,
  onToggle,
  onClear,
  onBuy,
  onShow,
  onClose,
}: {
  land: LandState
  selection: string[]
  quote: LandQuote | null
  onToggle: (id: string) => void
  onClear: () => void
  onBuy: () => void
  onShow: (parcel: Parcel) => void
  onClose: () => void
}) {
  return (
    <section className="land-panel" aria-label="Land">
      <div className="window-title">
        <span>
          Land · {land.owned_tiles}/{land.total_tiles} tiles owned
        </span>
        <button onClick={onClose} aria-label="Close land">
          ×
        </button>
      </div>
      {selection.length > 0 && quote && (
        <div className={`land-quote ${quote.ok ? "valid" : "invalid"}`} role="status">
          <span data-land-total={quote.total_cents}>
            <b>Selected: {money(quote.total_cents)}</b>
            {quote.ok ? ` · cash after ${money(quote.cash_after_cents)}` : ` · ${quote.message}`}
          </span>
          <div className="land-actions">
            <button data-land-buy disabled={!quote.ok} onClick={onBuy}>
              Buy {selection.length === 1 ? "parcel" : `${selection.length} parcels`}
            </button>
            <button onClick={onClear}>Clear</button>
          </div>
        </div>
      )}
      {land.parcels.length === 0 ? (
        <p className="guest-thought">This park owns its whole map; there is no land for sale.</p>
      ) : (
        <ul className="land-parcels">
          {land.parcels.map((parcel) => {
            const selected = selection.includes(parcel.id)
            return (
              <li key={parcel.id} className={`land-parcel ${parcel.status}`}>
                <button
                  className={`shop-row${selected ? " active" : ""}`}
                  data-parcel={parcel.id}
                  aria-pressed={selected}
                  disabled={parcel.owned}
                  onClick={() => onToggle(parcel.id)}
                >
                  <span>
                    <b>
                      {checkMark(parcel, selected)}
                      {parcel.title}
                    </b>
                    <small className="land-facts">{facts(parcel)}</small>
                    <small>{parcel.summary}</small>
                    {parcel.restriction && (
                      <small className="land-restriction">{parcel.restriction}</small>
                    )}
                  </span>
                  <strong>
                    {parcel.owned ? STATUS_LABEL.owned : money(parcel.price_cents)}
                    <small className="land-status" data-status={parcel.status}>
                      {parcel.owned ? "" : STATUS_LABEL[parcel.status]}
                    </small>
                  </strong>
                </button>
                <button
                  className="land-show"
                  aria-label={`Show ${parcel.title}`}
                  onClick={() => onShow(parcel)}
                >
                  ⌖
                </button>
              </li>
            )
          })}
        </ul>
      )}
      {selection.length === 0 && land.parcels.some((parcel) => !parcel.owned) && (
        <p className="guest-thought">
          Tap a parcel on the map or in the list to compare it; purchases are final.
        </p>
      )}
    </section>
  )
}

export function PlanPanel({
  plan,
  land,
  planning,
  onBuild,
  onRemove,
  onClear,
}: {
  plan: PlanEvaluation[]
  land: LandState
  planning: boolean
  onBuild: (item: PlanItem) => void
  onRemove: (item: PlanItem) => void
  onClear: () => void
}) {
  const construction = plan.reduce((sum, entry) => sum + entry.cost_cents, 0)
  const needed = [...new Set(plan.flatMap((entry) => entry.land_parcels))].flatMap((id) => {
    const parcel = land.parcels.find((candidate) => candidate.id === id)
    return parcel ? [parcel] : []
  })
  const landCost = needed.reduce((sum, parcel) => sum + parcel.price_cents, 0)
  return (
    <section className="plan-panel" aria-label="Expansion plan">
      <div className="window-title">
        <span>Plan · nothing is spent until you build</span>
        <button onClick={onClear} aria-label="Clear plan">
          ×
        </button>
      </div>
      <ul className="plan-items">
        {plan.map((entry) => (
          <li
            key={entry.item.id}
            className={entry.ok ? "valid" : "invalid"}
            data-plan-item={entry.item.id}
          >
            <span>
              <b>
                {entry.ok ? "✓" : "!"} {planLabel(entry.item)} · {money(entry.cost_cents)}
              </b>
              <small>{entry.message}</small>
            </span>
            <button disabled={!entry.ok} onClick={() => onBuild(entry.item)}>
              Build
            </button>
            <button aria-label="Remove from plan" onClick={() => onRemove(entry.item)}>
              ✕
            </button>
          </li>
        ))}
      </ul>
      <p className="plan-total" data-plan-total={construction + landCost}>
        <b>Plan total {money(construction + landCost)}</b> · construction {money(construction)}
        {needed.length > 0 &&
          ` · land ${money(landCost)} (${needed.map((parcel) => parcel.title).join(", ")})`}
      </p>
      {!planning && <small>Turn on “Plan” in the Path or Habitat tool to add more.</small>}
    </section>
  )
}
