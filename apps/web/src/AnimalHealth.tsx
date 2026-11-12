// Animal health (#109): presents zoo-core's health record for one animal — the warning, the
// gentle illness, the vet's treatment, quarantine and recovery. Every status, reason and
// preview comes from Rust; these components lay them out and send the player's choice back.
import type { AlertAction, Animal, QuarantinePreview } from "./game-types"

const STATUS_LABELS: Record<Animal["health"]["status"], string> = {
  healthy: "Healthy",
  at_risk: "At risk",
  unwell: "Unwell",
  recovering: "Recovering",
}

export function AnimalHealthPanel({
  animal,
  quarantine,
  fixLabel,
  onFix,
  onQuarantine,
}: {
  animal: Animal
  /** zoo-core's quarantine preview, when the animal is ill and not yet in quarantine. */
  quarantine: QuarantinePreview | null
  /** The button text for a treatment's suggested fix, if it has one. */
  fixLabel: (action: AlertAction) => string | null
  onFix: (action: AlertAction) => void
  onQuarantine: () => void
}) {
  const { health } = animal
  const action = health.treatment?.action ?? null
  const label = action ? fixLabel(action) : null
  if (health.status === "healthy" && !health.release_blocked) {
    return (
      <section className="animal-health health-healthy" aria-label="Health" data-status="healthy">
        <h3>
          Health <span className="health-badge">Healthy</span>
        </h3>
      </section>
    )
  }
  return (
    <section
      className={`animal-health health-${health.status}`}
      aria-label="Health"
      data-status={health.status}
    >
      <h3>
        Health <span className="health-badge">{STATUS_LABELS[health.status]}</span>
      </h3>
      <div className="guest-thought" data-testid="animal-health">
        {health.summary}
      </div>
      {health.status === "at_risk" && (
        <div className="need-row warning">
          <div>
            <span>Risk</span>
            <strong>{health.risk_percent}%</strong>
          </div>
          <div className="need-track">
            <span style={{ width: `${health.risk_percent}%` }} />
          </div>
        </div>
      )}
      {health.recovery_percent !== null && (
        <div className={`need-row ${health.recovery_paused ? "warning" : ""}`}>
          <div>
            <span>Recovery</span>
            <strong>{health.recovery_percent}%</strong>
          </div>
          <div className="need-track">
            <span style={{ width: `${health.recovery_percent}%` }} />
          </div>
        </div>
      )}
      {health.in_quarantine && (
        <small className="health-quarantine" data-testid="quarantine-status">
          In quarantine · returns to habitat #{health.quarantine_home} once recovered
        </small>
      )}
      <div className="animal-actions">
        {action && label && <button onClick={() => onFix(action)}>{label}</button>}
        {quarantine && (
          <button
            disabled={!quarantine.ok}
            onClick={onQuarantine}
            title={quarantine.message}
            data-testid="quarantine-button"
          >
            Move to quarantine
          </button>
        )}
      </div>
      {quarantine && !quarantine.ok && (
        <small className="arrival-blocked" data-testid="quarantine-blocked">
          ✕ {quarantine.message}
        </small>
      )}
    </section>
  )
}
