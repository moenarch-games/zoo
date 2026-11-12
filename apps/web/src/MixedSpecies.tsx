// Mixed-species habitats (#108): presents zoo-core's resident groups, adoption and transfer
// previews. Every compatibility verdict, capacity, cost and welfare effect comes from Rust;
// these components only lay the values out and send the player's choice back as a command.
import { useState } from "react"

import type { Animal, ArrivalPreview, Habitat, SpeciesKey, SpeciesOffer } from "./game-types"
import { money } from "./ManagementWorkspace"

/** The most animals one adoption brings, as zoo-core allows. */
const MAX_GROUP = 12

/** Each resident species with its own group, welfare and social fit. */
export function ResidentGroups({ habitat }: { habitat: Habitat }) {
  if (habitat.residents.length === 0) {
    return null
  }
  return (
    <ul className="resident-groups" aria-label="Resident species">
      {habitat.residents.map((resident) => (
        <li
          key={resident.species}
          data-species={resident.species}
          className={resident.social_score < 100 ? "unmet" : "met"}
        >
          <b>{resident.label}</b>
          <span>
            {resident.animals} of {resident.minimum_social_group}+ together
          </span>
          <span>
            Welfare {resident.welfare}% → {resident.welfare_target}%
          </span>
        </li>
      ))}
    </ul>
  )
}

function pairingPartner(preview: ArrivalPreview, species: SpeciesKey, offers: SpeciesOffer[]) {
  return preview.pairings.map((pairing) => {
    const other = pairing.species[0] === species ? pairing.species[1] : pairing.species[0]
    return {
      label: offers.find((offer) => offer.key === other)?.label ?? other,
      reason: pairing.reason,
    }
  })
}

/** The adoption list: Rust's verdict and effect for each species at the chosen group size. */
export function AdoptionOffers({
  offers,
  previews,
  count,
  onCount,
  onAdopt,
  glyph,
}: {
  offers: SpeciesOffer[]
  previews: Map<SpeciesKey, ArrivalPreview>
  count: number
  onCount: (count: number) => void
  onAdopt: (species: SpeciesKey) => void
  glyph: (assetRef: string) => string | undefined
}) {
  // A reason every species shares (an unstaffed or full habitat) is stated once.
  const reasons = new Set(
    [...previews.values()].map((preview) => (preview.ok ? "" : preview.message)),
  )
  const [only] = reasons
  const shared = previews.size > 0 && reasons.size === 1 && only ? only : null
  const setCount = (value: number) => {
    if (Number.isInteger(value)) {
      onCount(Math.min(MAX_GROUP, Math.max(1, value)))
    }
  }
  return (
    <>
      <div className="adopt-count">
        <span id="adopt-count-label">Group size</span>
        <button
          aria-label="Fewer animals"
          disabled={count <= 1}
          onClick={() => setCount(count - 1)}
        >
          −
        </button>
        <input
          type="number"
          min={1}
          max={MAX_GROUP}
          step={1}
          value={count}
          aria-labelledby="adopt-count-label"
          onChange={(event) => setCount(Number.parseInt(event.target.value, 10))}
        />
        <button
          aria-label="More animals"
          disabled={count >= MAX_GROUP}
          onClick={() => setCount(count + 1)}
        >
          +
        </button>
      </div>
      {shared && <small className="arrival-blocked">✕ {shared}</small>}
      {offers.map((offer) => {
        const preview = previews.get(offer.key)
        const blocked = preview !== undefined && !preview.ok
        const destination = preview?.destination ?? null
        // The verdict sits beside the button, so a button's text stays the species it adopts.
        return (
          <div className="species-choice" key={offer.key} data-species={offer.key}>
            <button
              className="shop-row species-row"
              disabled={preview === undefined || blocked}
              onClick={() => onAdopt(offer.key)}
            >
              <span className="species-offer">
                <i>{glyph(offer.asset_ref)}</i>
                <span>
                  <b>{offer.label}</b>
                  <small>
                    Group {offer.minimum_social_group}+ · {offer.space_per_animal} space each ·
                    fence {offer.minimum_fence_height_cm} cm+
                  </small>
                </span>
              </span>
              <strong>{money(preview?.cost_cents ?? offer.purchase_cost_cents * count)}</strong>
            </button>
            {preview && !shared && (blocked || (destination?.animals_before ?? 0) > 0) && (
              <div className="arrival-preview" data-testid="arrival-preview">
                {blocked && <small className="arrival-blocked">✕ {preview.message}</small>}
                {!blocked &&
                  pairingPartner(preview, offer.key, offers).map((partner) => (
                    <small className="arrival-pairing" key={partner.label}>
                      Mixes with {partner.label}: {partner.reason}
                    </small>
                  ))}
                {!blocked && destination && destination.animals_before > 0 && (
                  <small className="arrival-effect">
                    Space {destination.space.needed_after}/{destination.space.provided} · feed use{" "}
                    {destination.needs_per_hour_before}→{destination.needs_per_hour_after}/h ·
                    upkeep {money(destination.upkeep_cents_per_hour_after)}/h
                  </small>
                )}
                {!blocked &&
                  preview.warnings.map((warning) => (
                    <small className="arrival-warning" key={warning}>
                      ! {warning}
                    </small>
                  ))}
              </div>
            )}
          </div>
        )
      })}
    </>
  )
}

/** Moving an animal to another habitat, or sending it to a partner zoo. */
export function AnimalMoves({
  animal,
  habitats,
  speciesLabels,
  target,
  preview,
  onTarget,
  onTransfer,
  onRehome,
}: {
  animal: Animal
  habitats: Habitat[]
  speciesLabels: (habitat: Habitat) => string
  target: number | null
  preview: ArrivalPreview | null
  onTarget: (habitatId: number | null) => void
  onTransfer: () => void
  onRehome: () => void
}) {
  const [confirming, setConfirming] = useState(false)
  const others = habitats.filter((habitat) => habitat.id !== animal.habitat_id)
  return (
    <section className="animal-moves" aria-label="Move animal">
      <h3>Move</h3>
      <label>
        <span>Move to</span>
        <select
          aria-label="Destination habitat"
          value={target ?? ""}
          onChange={(event) =>
            onTarget(event.target.value === "" ? null : Number(event.target.value))
          }
        >
          <option value="">Choose a habitat</option>
          {others.map((habitat) => (
            <option key={habitat.id} value={habitat.id}>
              Habitat #{habitat.id} · {speciesLabels(habitat)} · {habitat.animals}/
              {habitat.capacity}
            </option>
          ))}
        </select>
      </label>
      {preview && (
        <div className="arrival-preview" data-testid="transfer-preview">
          <div className={preview.ok ? "guest-thought" : "guest-thought arrival-blocked"}>
            {preview.ok ? preview.message : `✕ ${preview.message}`}
          </div>
          {preview.warnings.map((warning) => (
            <small className="arrival-warning" key={warning}>
              ! {warning}
            </small>
          ))}
        </div>
      )}
      <div className="animal-actions">
        <button disabled={!preview?.ok} onClick={onTransfer}>
          Move here
        </button>
        {confirming ? (
          <button
            className="danger"
            onClick={() => {
              setConfirming(false)
              onRehome()
            }}
          >
            Confirm: {animal.name} leaves
          </button>
        ) : (
          <button onClick={() => setConfirming(true)}>Send to partner zoo</button>
        )}
      </div>
    </section>
  )
}
