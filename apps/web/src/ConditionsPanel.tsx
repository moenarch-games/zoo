// The weather and events report (#118): today's season and weather and what it does, the
// forecast, the announced event and recent ones, the player's rules and the display settings.
// Every condition, effect and event comes from zoo-core; the display settings belong to the
// browser and change only how the park looks, never what happens in it.
import type { ConditionRule, ConditionsState } from "./game-types"
/** "HH:MM" of a minute of the day. */
function clock(minute: number) {
  return `${String(Math.floor(minute / 60)).padStart(2, "0")}:${String(minute % 60).padStart(2, "0")}`
}

/** Browser-only display settings; switching them off never changes an outcome. */
export type DisplaySettings = {
  /** Light the park by the simulation's time of day; off keeps a constant daylight. */
  daylight: boolean
  /** Tint the park for the day's weather; off shows every day in neutral light. */
  weatherVisuals: boolean
}

export const DEFAULT_DISPLAY: DisplaySettings = { daylight: true, weatherVisuals: true }

const DISPLAY_KEY = "zoo.display"

export function loadDisplaySettings(): DisplaySettings {
  try {
    const stored = JSON.parse(window.localStorage.getItem(DISPLAY_KEY) ?? "null") as unknown
    if (stored && typeof stored === "object") {
      const value = stored as Partial<DisplaySettings>
      return {
        daylight: value.daylight !== false,
        weatherVisuals: value.weatherVisuals !== false,
      }
    }
  } catch (error) {
    console.warn("Zoo display settings could not be read; using defaults", error)
  }
  return DEFAULT_DISPLAY
}

export function storeDisplaySettings(settings: DisplaySettings) {
  try {
    window.localStorage.setItem(DISPLAY_KEY, JSON.stringify(settings))
  } catch (error) {
    console.warn("Zoo display settings could not be stored", error)
  }
}

const WEATHER_ICON = { fair: "☀", heat: "♨" } as const

const PHASE_LABELS = { night: "Night", dawn: "Dawn", day: "Day", dusk: "Dusk" } as const

/** The compact top-bar label: season and weather. */
export function conditionsLabel(conditions: ConditionsState) {
  return `${WEATHER_ICON[conditions.weather]} ${conditions.season_label}`
}

const RULES: { rule: ConditionRule; label: string; detail: string }[] = [
  {
    rule: "weather_effects",
    label: "Weather effects",
    detail: "Hot days make animals without shade less comfortable and guests thirstier.",
  },
  {
    rule: "events",
    label: "Park events",
    detail: "Occasional announced events such as a school visit.",
  },
]

export function ConditionsPanel({
  conditions,
  day,
  onRule,
  display,
  onDisplay,
}: {
  conditions: ConditionsState
  day: number
  onRule: (rule: ConditionRule, enabled: boolean) => void
  display: DisplaySettings
  onDisplay: (settings: DisplaySettings) => void
}) {
  const scheduled = conditions.scheduled
  return (
    <div className="conditions-panel" data-weather={conditions.weather}>
      <section className="conditions-today" aria-label="Today's conditions">
        <h3>
          Day {day} · {conditions.season_label} (day {conditions.season_day} of{" "}
          {conditions.season_days})
        </h3>
        <p className="conditions-summary" data-active={conditions.effects_active}>
          <span aria-hidden="true">{WEATHER_ICON[conditions.weather]}</span> {conditions.summary}
        </p>
        <p className="conditions-light">
          {PHASE_LABELS[conditions.light.phase]} · sunrise {clock(conditions.light.sunrise_minute)}{" "}
          · sunset {clock(conditions.light.sunset_minute)}
        </p>
      </section>
      {conditions.weather_stream ? (
        <ol className="forecast" aria-label="Forecast">
          {conditions.forecast.map((entry) => (
            <li key={entry.day} data-day={entry.day} data-weather={entry.weather}>
              <span>{entry.day === day ? "Today" : `Day ${entry.day}`}</span>
              <strong>
                <span aria-hidden="true">{WEATHER_ICON[entry.weather]}</span> {entry.label}
              </strong>
              <small>{entry.season_label}</small>
            </li>
          ))}
        </ol>
      ) : (
        <p className="conditions-note">This park's weather is always fair.</p>
      )}
      {(conditions.events_defined || conditions.history.length > 0) && (
        <section className="park-events" aria-label="Park events">
          <h3>Events</h3>
          {scheduled ? (
            <p className="event-scheduled" data-day={scheduled.day}>
              <strong>{scheduled.label}</strong> {scheduled.message}
            </p>
          ) : (
            <p className="conditions-note">No event is announced.</p>
          )}
          {conditions.history.length > 0 && (
            <ul className="event-history">
              {conditions.history.map((record) => (
                <li
                  key={`${record.kind}:${record.day}`}
                  data-outcome={record.outcome}
                  data-day={record.day}
                >
                  {record.message}
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
      <fieldset className="condition-rules">
        <legend>Rules</legend>
        {RULES.map(({ rule, label, detail }) => (
          <label key={rule}>
            <input
              type="checkbox"
              checked={conditions.rules[rule]}
              onChange={(event) => onRule(rule, event.target.checked)}
            />
            <span>
              {label}
              <small>{detail}</small>
            </span>
          </label>
        ))}
      </fieldset>
      <fieldset className="display-settings">
        <legend>Display</legend>
        <label>
          <input
            type="checkbox"
            checked={display.daylight}
            onChange={(event) => onDisplay({ ...display, daylight: event.target.checked })}
          />
          <span>
            Day and night lighting<small>Off keeps the park in steady daylight.</small>
          </span>
        </label>
        <label>
          <input
            type="checkbox"
            checked={display.weatherVisuals}
            onChange={(event) => onDisplay({ ...display, weatherVisuals: event.target.checked })}
          />
          <span>
            Weather visuals
            <small>Off shows every day in neutral light; the weather still applies.</small>
          </span>
        </label>
      </fieldset>
    </div>
  )
}
