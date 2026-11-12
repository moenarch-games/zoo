// Utilities (#113): presents zoo-core's pool-water and climate allocation for a habitat and a
// plant's capacity, consumers and failure state. Every number, order, reason and fix comes
// from Rust; these components lay them out and send the player's choice back.
import type {
  Concession,
  Habitat,
  UtilityFacility,
  UtilityKind,
  UtilityPriority,
  UtilitySupply,
} from "./game-types"
import { money } from "./ManagementWorkspace"

const KINDS: UtilityKind[] = ["water", "climate"]
const PRIORITIES: UtilityPriority[] = ["high", "normal", "low"]
const PRIORITY_LABELS: Record<UtilityPriority, string> = {
  high: "High",
  normal: "Normal",
  low: "Low",
}
const STATUS_LABELS: Record<UtilitySupply["status"], string> = {
  not_needed: "Not needed",
  supplied: "Supplied",
  short: "Short",
  degraded: "Degraded",
}

export function HabitatUtilitiesPanel({
  habitat,
  plants,
  onPriority,
  onReplace,
  onOpenPlant,
}: {
  habitat: Habitat
  /** Every utility plant in the park, to name and replace the ones this habitat reaches. */
  plants: Concession[]
  onPriority: (priority: UtilityPriority) => void
  onReplace: (plantId: number) => void
  onOpenPlant: (plantId: number) => void
}) {
  const { utilities } = habitat
  const needed = KINDS.map((kind) => utilities[kind]).filter((supply) => supply.demand > 0)
  if (needed.length === 0) {
    return null
  }
  const worst =
    (["degraded", "short"] as const).find((status) =>
      needed.some((supply) => supply.status === status),
    ) ?? "supplied"
  return (
    <section
      className={`habitat-utilities utilities-${worst}`}
      aria-label="Utilities"
      data-testid="habitat-utilities"
      data-status={worst}
    >
      <h3>Utilities</h3>
      {needed.map((supply) => {
        const failed = supply.reachable
          .map((id) => plants.find((plant) => plant.id === id))
          .filter((plant): plant is Concession => plant?.utility?.working === false)
        return (
          <div
            key={supply.kind}
            className={`utility-supply ${supply.status === "supplied" ? "" : "zone-warning"}`}
            data-kind={supply.kind}
            data-status={supply.status}
          >
            <div className="need-row">
              <div>
                <span>{supply.label}</span>
                <strong>
                  {supply.supplied}/{supply.demand} · {STATUS_LABELS[supply.status]}
                </strong>
              </div>
              <div className="need-track">
                <span
                  style={{ width: `${(100 * supply.supplied) / Math.max(supply.demand, 1)}%` }}
                />
              </div>
            </div>
            <small data-testid={`utility-${supply.kind}-detail`}>
              {supply.detail}
              {supply.rank > 0 ? ` · served #${supply.rank} in order` : ""}
            </small>
            {supply.fix && <small className="utility-fix">{supply.fix}</small>}
            {supply.draws.length > 0 && (
              <div className="utility-draws">
                {supply.draws.map((draw) => (
                  <button key={draw.concession_id} onClick={() => onOpenPlant(draw.concession_id)}>
                    Plant #{draw.concession_id} · {draw.units} units
                  </button>
                ))}
              </div>
            )}
            {failed.map((plant) => (
              <button
                key={plant.id}
                className="shop-row"
                data-testid="replace-failed-plant"
                onClick={() => onReplace(plant.id)}
              >
                <span>
                  <b>Replace plant #{plant.id}</b>
                  <small>Restores supply now; a mechanic's repair is cheaper but takes time</small>
                </span>
                <strong>{money(plant.utility?.replacement_cents ?? plant.build_cost_cents)}</strong>
              </button>
            ))}
          </div>
        )
      })}
      <label className="utility-priority">
        <span>Utility priority</span>
        <select
          value={utilities.priority}
          onChange={(event) => onPriority(event.target.value as UtilityPriority)}
        >
          {PRIORITIES.map((priority) => (
            <option key={priority} value={priority}>
              {PRIORITY_LABELS[priority]}
            </option>
          ))}
        </select>
      </label>
    </section>
  )
}

/** A plant's rows in the facility inspector. */
export function UtilityPlantRows({ utility }: { utility: UtilityFacility }) {
  return (
    <>
      <div className={utility.working && !utility.overloaded ? "" : "zone-warning"}>
        <dt>Supply</dt>
        <dd data-testid="utility-plant-status">{utility.status}</dd>
      </div>
      <div>
        <dt>Capacity</dt>
        <dd>
          {utility.allocated}/{utility.capacity} units in use · reaches {utility.coverage_tiles}{" "}
          path tiles
        </dd>
      </div>
      <div>
        <dt>Serves</dt>
        <dd>
          {utility.consumers.length > 0
            ? utility.consumers
                .map(
                  (consumer) =>
                    `#${consumer.rank} habitat ${consumer.habitat_id} (${consumer.units}, ${consumer.priority})`,
                )
                .join(" · ")
            : "No habitat"}
        </dd>
      </div>
      <div>
        <dt>Running cost</dt>
        <dd>
          {utility.working ? `${money(utility.hourly_operating_cents)}/h` : "None while failed"}
        </dd>
      </div>
    </>
  )
}

export function UtilityPlantActions({
  utility,
  onReplace,
}: {
  utility: UtilityFacility
  onReplace: () => void
}) {
  return (
    <button
      className="shop-row"
      data-testid="replace-plant"
      disabled={!utility.can_replace}
      onClick={onReplace}
    >
      <span>
        <b>Replace plant</b>
        <small>
          {utility.can_replace
            ? "A new plant at full condition, withdrawing its repair job"
            : "Only worn or failed plants can be replaced"}
        </small>
      </span>
      <strong>{money(utility.replacement_cents)}</strong>
    </button>
  )
}
