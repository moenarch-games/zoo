// Habitat design (#97): presents zoo-core's environment, requirements and welfare breakdown and
// gathers the brush the terrain tool paints with. Every rule, price and reachability decision
// comes from Rust; this panel only lays the values out.
import type {
  EnvironmentCatalog,
  EnvironmentPreview,
  Habitat,
  HabitatRequirements,
  Surface,
} from "./game-types"
import { money } from "./ManagementWorkspace"

export type HabitatBrush =
  | { kind: "surface"; surface: Surface }
  | { kind: "height"; height: number }
  | { kind: "object"; key: string }

export function brushLabel(brush: HabitatBrush, catalog: EnvironmentCatalog) {
  switch (brush.kind) {
    case "surface":
      return (
        catalog.surfaces.find((surface) => surface.key === brush.surface)?.label ?? brush.surface
      )
    case "height":
      return `height ${brush.height}`
    case "object":
      return catalog.objects.find((object) => object.key === brush.key)?.label ?? brush.key
  }
}

function requirementSummary(needs: HabitatRequirements) {
  const parts = [`${needs.ground_cover_percent}% ${needs.ground_cover.replace("_", " ")}`]
  if (needs.shallow_water_tiles > 0) {
    parts.push(`${needs.shallow_water_tiles} shallow water`)
  }
  if (needs.deep_water_tiles > 0) {
    parts.push(`${needs.deep_water_tiles} deep water`)
  }
  if (needs.shelter) {
    parts.push("shelter")
  }
  if (needs.enrichment) {
    parts.push("enrichment")
  }
  return parts.join(" · ")
}

export function HabitatDesignPanel({
  habitat,
  catalog,
  brush,
  designing,
  preview,
  onBrush,
  onToggleDesign,
  onRemoveObject,
}: {
  habitat: Habitat
  catalog: EnvironmentCatalog
  brush: HabitatBrush
  designing: boolean
  preview: EnvironmentPreview | null
  onBrush: (brush: HabitatBrush) => void
  onToggleDesign: () => void
  onRemoveObject: (objectId: number) => void
}) {
  const { assessment } = habitat.environment
  const active = (candidate: HabitatBrush) =>
    candidate.kind === brush.kind &&
    JSON.stringify(candidate) === JSON.stringify(brush) &&
    designing
  return (
    <section className="habitat-design" aria-label="Habitat design">
      <h3>Habitat design</h3>
      {habitat.requirements && (
        <div className="guest-thought">Needs {requirementSummary(habitat.requirements)}</div>
      )}
      <dl>
        <div>
          <dt>Usable dry land</dt>
          <dd>
            {assessment.usable_land} tiles
            {assessment.unreachable_tiles > 0 && ` · ${assessment.unreachable_tiles} cut off`}
          </dd>
        </div>
        <div>
          <dt>Water</dt>
          <dd>
            {assessment.shallow_water} shallow · {assessment.deep_water} deep
          </dd>
        </div>
        <div>
          <dt>Shelter / enrichment</dt>
          <dd>
            for {assessment.shelter_capacity} / {assessment.enrichment_capacity} animals
          </dd>
        </div>
      </dl>
      <ul className="welfare-parts" aria-label="Welfare breakdown">
        {habitat.welfare_parts.map((part) => (
          <li key={part.key} className={part.met ? "met" : "unmet"} data-part={part.key}>
            <span>
              <b>
                {part.met ? "✓" : "!"} {part.label}
              </b>{" "}
              <small>
                {part.score}% ×{part.weight}
              </small>
            </span>
            <small>{part.detail}</small>
          </li>
        ))}
      </ul>

      <button
        className={`shop-row${designing ? " active" : ""}`}
        aria-pressed={designing}
        onClick={onToggleDesign}
      >
        <span>
          <b>{designing ? "Stop designing" : "Design terrain"}</b>
          <small>
            {designing
              ? `Drag inside the fence to apply ${brushLabel(brush, catalog)}`
              : "Paint ground and water, set heights, place furnishings"}
          </small>
        </span>
      </button>
      {designing && (
        <div className="design-brushes">
          <div role="group" aria-label="Surface">
            {catalog.surfaces.map((surface) => (
              <button
                key={surface.key}
                className={active({ kind: "surface", surface: surface.key }) ? "active" : ""}
                onClick={() => onBrush({ kind: "surface", surface: surface.key })}
              >
                {surface.label} <small>{money(surface.tile_cents)}/tile</small>
              </button>
            ))}
          </div>
          <label className="design-height">
            Height (0–{catalog.max_height})
            <input
              type="number"
              min={0}
              max={catalog.max_height}
              step={1}
              value={brush.kind === "height" ? brush.height : 0}
              onChange={(event) => {
                const height = Math.max(
                  0,
                  Math.min(catalog.max_height, Math.round(Number(event.target.value) || 0)),
                )
                onBrush({ kind: "height", height })
              }}
              onFocus={() => {
                if (brush.kind !== "height") {
                  onBrush({ kind: "height", height: 0 })
                }
              }}
            />
            <small>
              {money(catalog.elevation_step_cents)} per step · animals climb {catalog.max_walk_step}{" "}
              step · neighbours differ by at most {catalog.max_slope_step} · fence-side tiles and
              water stay at 0
            </small>
          </label>
          <div role="group" aria-label="Furnishings">
            {catalog.objects.map((object) => (
              <button
                key={object.key}
                className={active({ kind: "object", key: object.key }) ? "active" : ""}
                onClick={() => onBrush({ kind: "object", key: object.key })}
              >
                {object.label}{" "}
                <small>
                  {object.role} for {object.capacity} · {money(object.build_cents)}
                </small>
              </button>
            ))}
          </div>
          {preview && (
            <div className={`guest-thought ${preview.ok ? "" : "zone-warning"}`}>
              {preview.message}
              {preview.ok && preview.cost_cents > 0 && ` · ${money(preview.cost_cents)}`}
              {preview.ok &&
                preview.welfare_target !== null &&
                ` · welfare target ${habitat.welfare_target}% → ${preview.welfare_target}%`}
            </div>
          )}
        </div>
      )}
      {habitat.environment.objects.length > 0 && (
        <ul className="habitat-objects" aria-label="Furnishings">
          {habitat.environment.objects.map((object) => (
            <li key={object.id}>
              <span>
                {object.label} #{object.id} at {object.x},{object.y}
                {object.reachable ? "" : " · unreachable, no benefit"}
              </span>
              <button onClick={() => onRemoveObject(object.id)}>Remove</button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
