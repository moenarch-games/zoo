// The education and conservation report (#116): keeper talks and their places, what signs
// teach, what guests learned and donated, conservation milestones and scenario grants. Every
// status, reason and figure comes from zoo-core; this file keeps only unfinished talk drafts
// and lays the report out.
import { useId, useState } from "react"

import type { EducationState, TalkState } from "./game-types"
import { money, parkTime } from "./ManagementWorkspace"

export type EducationCommands = {
  scheduleTalk: (habitatId: number, startMinute: number, duration: number, places: number) => void
  cancelTalk: (habitatId: number) => void
}

export type TalkHabitat = { id: number; label: string; stocked: boolean }

const TALK_STATUS: Record<TalkState["status"], string> = {
  scheduled: "Scheduled",
  waiting_for_keeper: "Waiting for a keeper",
  keeper_on_the_way: "Keeper on the way",
  in_progress: "Talking",
  blocked: "Blocked",
}

const SIGN_STATUS = {
  teaching: "Teaching",
  decorative: "Decorative",
  unreachable: "Unreachable",
} as const

function clock(minuteOfDay: number) {
  return `${String(Math.floor(minuteOfDay / 60)).padStart(2, "0")}:${String(minuteOfDay % 60).padStart(2, "0")}`
}

/** "HH:MM" as a minute of the day, or `null`. */
function parseClock(text: string): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(text.trim())
  if (!match) {
    return null
  }
  const minute = Number(match[1]) * 60 + Number(match[2])
  return minute < 1440 ? minute : null
}

/** One habitat's talk: its schedule form and, once scheduled, where today's talk stands. */
function TalkRow({
  habitat,
  talk,
  commands,
}: {
  habitat: TalkHabitat
  talk: TalkState | undefined
  commands: EducationCommands
}) {
  const id = useId()
  const [start, setStart] = useState(talk ? clock(talk.start_minute_of_day) : "11:00")
  const [duration, setDuration] = useState(String(talk?.duration_minutes ?? 20))
  const [places, setPlaces] = useState(String(talk?.places ?? 6))
  const startMinute = parseClock(start)
  const durationMinutes = Number.parseInt(duration, 10)
  const placeCount = Number.parseInt(places, 10)
  const valid =
    startMinute !== null && Number.isFinite(durationMinutes) && Number.isFinite(placeCount)
  return (
    <li className="talk-row" data-habitat={habitat.id} data-status={talk?.status ?? "none"}>
      <div className="talk-head">
        <strong>
          Habitat #{habitat.id} · {habitat.label}
        </strong>
        {talk && (
          <span className={`talk-status ${talk.status}`}>
            {TALK_STATUS[talk.status]}
            {talk.status === "in_progress" || talk.status === "keeper_on_the_way"
              ? ` · ${talk.attendees}/${talk.places}`
              : ""}
          </span>
        )}
      </div>
      {talk && <p className="talk-detail">{talk.detail}</p>}
      {talk?.last && (
        <p className="talk-last" data-given={String(talk.last.given)}>
          Last talk {parkTime(talk.last.minute)}: {talk.last.detail}
        </p>
      )}
      <form
        className="talk-form"
        onSubmit={(event) => {
          event.preventDefault()
          if (valid) {
            commands.scheduleTalk(habitat.id, startMinute, durationMinutes, placeCount)
          }
        }}
      >
        <label htmlFor={`${id}-start`}>Daily at</label>
        <input
          id={`${id}-start`}
          type="text"
          inputMode="numeric"
          placeholder="HH:MM"
          autoComplete="off"
          value={start}
          data-talk-input="start"
          onChange={(event) => setStart(event.target.value)}
        />
        <label htmlFor={`${id}-duration`}>Minutes</label>
        <input
          id={`${id}-duration`}
          type="number"
          inputMode="numeric"
          min={10}
          max={60}
          value={duration}
          data-talk-input="duration"
          onChange={(event) => setDuration(event.target.value)}
        />
        <label htmlFor={`${id}-places`}>Places</label>
        <input
          id={`${id}-places`}
          type="number"
          inputMode="numeric"
          min={1}
          max={12}
          value={places}
          data-talk-input="places"
          onChange={(event) => setPlaces(event.target.value)}
        />
        <button type="submit" className="primary" disabled={!valid || !habitat.stocked}>
          {talk ? "Reschedule talk" : "Schedule talk"}
        </button>
        {talk && (
          <button type="button" onClick={() => commands.cancelTalk(habitat.id)}>
            Cancel talk
          </button>
        )}
      </form>
    </li>
  )
}

export function EducationPanel({
  education,
  habitats,
  commands,
}: {
  education: EducationState
  habitats: TalkHabitat[]
  commands: EducationCommands
}) {
  const { today, total } = education.learning
  return (
    <div
      className="education-panel"
      data-educated={total.guests_educated}
      data-talks-given={total.talks_given}
      data-donations={total.donations_cents}
      data-grants={total.grants_cents}
      data-learners={education.learning.learners_now}
    >
      <section aria-label="Learning">
        <h3>Learning</h3>
        <dl className="education-figures">
          <div>
            <dt>Guests taught</dt>
            <dd>
              {today.guests_educated} today · {total.guests_educated} in all
            </dd>
          </div>
          <div>
            <dt>Learning now</dt>
            <dd>
              {education.learning.learners_now} guests · average{" "}
              {education.learning.average_learning}/100
            </dd>
          </div>
          <div>
            <dt>Talks</dt>
            <dd>
              {total.talks_given} given · {total.attendees} listeners · {total.talks_canceled}{" "}
              canceled
            </dd>
          </div>
          <div>
            <dt>Donations</dt>
            <dd>
              {money(today.donations_cents)} today · {money(total.donations_cents)} from{" "}
              {total.donors} {total.donors === 1 ? "donor" : "donors"}
            </dd>
          </div>
        </dl>
        {education.reasons.length > 0 && (
          <ul className="education-reasons">
            {education.reasons.map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Keeper talks">
        <h3>Keeper talks</h3>
        {habitats.length === 0 ? (
          <p className="empty-state">Build a habitat beside a public path to give talks.</p>
        ) : (
          <ul className="talk-list">
            {habitats.map((habitat) => (
              <TalkRow
                key={habitat.id}
                habitat={habitat}
                talk={education.talks.find((talk) => talk.habitat_id === habitat.id)}
                commands={commands}
              />
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Education signs">
        <h3>Signs</h3>
        {education.signs.length === 0 ? (
          <p className="empty-state">No signs yet: build one beside a path next to a habitat.</p>
        ) : (
          <ul className="sign-list">
            {education.signs.map((sign) => (
              <li key={sign.id} data-sign={sign.id} data-status={sign.status}>
                <strong>
                  Sign #{sign.id} · {SIGN_STATUS[sign.status]}
                </strong>
                <p>{sign.detail}</p>
                {sign.facts.map((fact) => (
                  <p key={fact.species} className="sign-fact">
                    “{fact.fact}”{" "}
                    <a href={fact.source_url} target="_blank" rel="noreferrer">
                      {fact.source_title}
                    </a>
                  </p>
                ))}
              </li>
            ))}
          </ul>
        )}
      </section>

      {education.donations.length > 0 && (
        <section aria-label="Donations by exhibit">
          <h3>Donations by exhibit</h3>
          <ul className="donation-list">
            {education.donations.map((source) => (
              <li key={source.habitat_id} data-habitat={source.habitat_id}>
                {source.label} · {money(source.cents)} from {source.donors}
              </li>
            ))}
          </ul>
        </section>
      )}

      <section aria-label="Conservation milestones">
        <h3>Conservation milestones</h3>
        <ul className="milestone-list">
          {education.achievements.map((achievement) => (
            <li
              key={achievement.kind}
              data-milestone={achievement.kind}
              data-earned={String(achievement.earned !== null)}
            >
              <strong>
                {achievement.earned ? "✓ " : ""}
                {achievement.title}
              </strong>
              <small>{achievement.description}</small>
              <p>{achievement.progress}</p>
            </li>
          ))}
        </ul>
        <small className="education-note">
          In-game milestones; no real-world impact is claimed.
        </small>
      </section>

      {education.grants.length > 0 && (
        <section aria-label="Grants">
          <h3>Grants</h3>
          <ul className="grant-list">
            {education.grants.map((grant) => (
              <li
                key={grant.id}
                data-grant={grant.id}
                data-awarded={String(grant.awarded_minute !== null)}
              >
                <strong>
                  {grant.title} · {money(grant.amount_cents)}
                </strong>
                <small>When {grant.condition}</small>
                <p>{grant.progress}</p>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  )
}
