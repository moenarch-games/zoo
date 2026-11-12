// The guest inspector's visit section (#103): group, profile, plan, habitats seen, crowding and
// exit reason, exactly as zoo-core reports them. Nothing here decides anything.
import type { ExitReason, Guest } from "./game-types"

export const EXIT_REASON_LABELS: Record<ExitReason, string> = {
  satisfied: "Satisfied",
  needs_unmet: "Needs unmet",
  too_crowded: "Too crowded",
  poor_value: "Poor value",
  no_reachable_attraction: "Nothing more to reach",
}

export const EXIT_REASONS = Object.keys(EXIT_REASON_LABELS) as ExitReason[]

export function GuestVisit({ guest }: { guest: Guest }) {
  const group = guest.group
  return (
    <section
      className="guest-visit"
      aria-label="Visit"
      data-guest-id={guest.id}
      data-guest-state={guest.state}
      data-guest-group={group?.id ?? ""}
      data-guest-group-size={group?.members.length ?? 0}
      data-guest-waiting={group?.waiting_for_group ? "true" : "false"}
      data-guest-visited={guest.visited_habitats.join(",")}
      data-guest-errand={guest.comfort.facility_id ?? ""}
      data-guest-exit={guest.exit?.reason ?? ""}
    >
      <h3>Visit</h3>
      <dl>
        {group && (
          <>
            <div>
              <dt>Group</dt>
              <dd>
                #{group.id} · {group.profile_label} · {group.members.length}{" "}
                {group.members.length === 1 ? "person" : "people"}
                {group.waiting_for_group ? ` · waiting ${group.waiting_minutes} min` : ""}
              </dd>
            </div>
            <div>
              <dt>Hopes to see</dt>
              <dd>
                {guest.visited_habitats.length} of {group.visit_goal} habitats
                {group.favorite_species ? ` · loves ${group.favorite_species}` : ""}
              </dd>
            </div>
            {group.plan && (
              <div>
                <dt>Plan</dt>
                <dd>{group.plan}</dd>
              </div>
            )}
          </>
        )}
        <div>
          <dt>Seen</dt>
          <dd>
            {guest.visited_habitats.length === 0
              ? "Nothing yet"
              : guest.visited_habitats.map((id) => `Habitat #${id}`).join(" → ")}
          </dd>
        </div>
        <div>
          <dt>Crowds</dt>
          <dd>
            {guest.crowd_minutes} min{group ? ` of ${group.patience} tolerated` : ""}
            {guest.crowded ? " · crowded now" : ""}
          </dd>
        </div>
        {guest.exit && (
          <div>
            <dt>Leaving</dt>
            <dd>
              {EXIT_REASON_LABELS[guest.exit.reason]} · {guest.exit.detail}
            </dd>
          </div>
        )}
      </dl>
    </section>
  )
}
