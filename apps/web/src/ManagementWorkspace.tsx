// The compact management workspace (#94): alerts that jump to their cause, the daily summary
// with drill-downs, and sortable/searchable views of habitats, guests, staff, facilities and
// finances. Every number shown here comes from the zoo-core snapshot; this file only sorts,
// filters and lays it out.
import { useId, useMemo, useState, type ReactNode } from "react"

import type {
  Alert,
  AlertSubject,
  FinanceBreakdown,
  OverlayKind,
  OverlayView,
  Point,
  Snapshot,
} from "./game-types"
import { OVERLAY_COLORS } from "./overlay-colors"

export type WorkspaceTab =
  | "alerts"
  | "summary"
  | "habitats"
  | "guests"
  | "staff"
  | "facilities"
  | "finances"

/** Something a row or alert can focus: the alert subjects, plus guests and work zones. */
export type JumpTarget = AlertSubject | { kind: "guest"; id: number } | { kind: "zone"; id: number }

export const OVERLAY_KINDS: { kind: OverlayKind; label: string }[] = [
  { kind: "access", label: "Access" },
  { kind: "welfare", label: "Welfare" },
  { kind: "staff", label: "Staff coverage" },
  { kind: "cleanliness", label: "Cleanliness" },
  { kind: "facilities", label: "Facilities" },
  { kind: "viewing", label: "Guest viewing" },
]

const TABS: { tab: WorkspaceTab; label: string }[] = [
  { tab: "alerts", label: "Alerts" },
  { tab: "summary", label: "Today" },
  { tab: "habitats", label: "Habitats" },
  { tab: "guests", label: "Guests" },
  { tab: "staff", label: "Staff" },
  { tab: "facilities", label: "Facilities" },
  { tab: "finances", label: "Finances" },
]

export function money(cents: number) {
  const sign = cents < 0 ? "-" : ""
  return `${sign}$${(Math.abs(cents) / 100).toLocaleString(undefined, {
    maximumFractionDigits: 0,
  })}`
}

/** A park minute (since day 1, 00:00) as "Day 2 08:15". */
export function parkTime(minute: number) {
  const day = Math.floor(minute / 1440) + 1
  const ofDay = minute % 1440
  const hh = String(Math.floor(ofDay / 60)).padStart(2, "0")
  const mm = String(ofDay % 60).padStart(2, "0")
  return `Day ${day} ${hh}:${mm}`
}

const SEVERITY: Record<Alert["severity"], { icon: string; label: string }> = {
  critical: { icon: "▲", label: "Critical" },
  warning: { icon: "●", label: "Warning" },
  notice: { icon: "○", label: "Notice" },
}

const LEVEL_ICON = { good: "✓", warn: "!", bad: "✕" } as const

export function AlertList({
  alerts,
  limit,
  onJump,
  onAction,
  actionLabel,
}: {
  alerts: Alert[]
  limit?: number
  onJump: (target: JumpTarget, focus: Point) => void
  onAction: (alert: Alert) => void
  actionLabel: (alert: Alert) => string | null
}) {
  if (alerts.length === 0) {
    return (
      <p className="empty-state" role="status">
        No alerts: every habitat, facility and path is working.
      </p>
    )
  }
  const shown = limit === undefined ? alerts : alerts.slice(0, limit)
  return (
    <ul className="alert-list" aria-label="Park alerts">
      {shown.map((alert) => {
        const severity = SEVERITY[alert.severity]
        const action = actionLabel(alert)
        const key = `${alert.code}:${alert.subject.kind}:${"id" in alert.subject ? alert.subject.id : ""}`
        return (
          <li key={key} className={`alert-item ${alert.severity}`} data-code={alert.code}>
            <span className="alert-severity" aria-label={severity.label} title={severity.label}>
              {severity.icon}
            </span>
            <div className="alert-body">
              <p>{alert.message}</p>
              <small>
                {severity.label} · since {parkTime(alert.first_minute)}
              </small>
              <div className="alert-actions">
                <button onClick={() => onJump(alert.subject, alert.focus)}>Show</button>
                {action && (
                  <button className="primary" onClick={() => onAction(alert)}>
                    {action}
                  </button>
                )}
              </div>
            </div>
          </li>
        )
      })}
      {limit !== undefined && alerts.length > limit && (
        <li className="alert-more">{alerts.length - limit} more in the workspace</li>
      )}
    </ul>
  )
}

function Drill({
  label,
  value,
  children,
  empty,
}: {
  label: string
  value: string
  children: ReactNode
  empty?: boolean
}) {
  return (
    <details className="summary-row">
      <summary>
        <span>{label}</span>
        <strong>{value}</strong>
      </summary>
      {empty ? <p className="empty-state">Nothing to show yet.</p> : children}
    </details>
  )
}

const COMPLAINT_LABELS = {
  hungry: "Hungry",
  thirsty: "Thirsty",
  tired: "Tired",
  poor_value: "Poor value",
} as const

const INCOME_LINES: [keyof FinanceBreakdown, string][] = [
  ["admissions_income_cents", "Admissions"],
  ["concession_income_cents", "Guest concessions"],
]

const EXPENSE_LINES: [keyof FinanceBreakdown, string][] = [
  ["construction_expense_cents", "Construction"],
  ["animal_purchase_expense_cents", "Animal purchases"],
  ["habitat_care_expense_cents", "Habitat care"],
  ["animal_feed_expense_cents", "Animal feed"],
  ["keeper_hiring_expense_cents", "Keeper hiring"],
  ["janitor_hiring_expense_cents", "Janitor hiring"],
  ["mechanic_hiring_expense_cents", "Mechanic hiring"],
  ["maintenance_repair_expense_cents", "Maintenance repairs"],
  ["park_upkeep_expense_cents", "Park upkeep"],
  ["keeper_wages_expense_cents", "Keeper wages"],
  ["janitor_wages_expense_cents", "Janitor wages"],
  ["mechanic_wages_expense_cents", "Mechanic wages"],
]

export function DailySummaryView({
  snapshot,
  onJump,
}: {
  snapshot: Snapshot
  onJump: (target: JumpTarget, focus: Point) => void
}) {
  const { summary } = snapshot
  const breakdown = snapshot.finance.current_day.breakdown
  const habitat = (id: number) => snapshot.habitats.find((entry) => entry.id === id)
  const guest = (id: number) => snapshot.guests.find((entry) => entry.id === id)
  const center = (id: number): Point => {
    const entry = habitat(id)
    return entry
      ? { x: entry.x + Math.floor(entry.width / 2), y: entry.y + Math.floor(entry.height / 2) }
      : snapshot.entrance
  }
  return (
    <div className="daily-summary">
      <p className="summary-headline">{summary.headline}</p>
      <Drill label={`Profit · day ${summary.day}`} value={money(summary.profit_cents)}>
        <div className="finance-grid">
          {INCOME_LINES.map(([key, label]) => (
            <FinanceLine key={key} label={label} cents={breakdown[key]} />
          ))}
          <span>Income</span>
          <strong>{money(summary.income_cents)}</strong>
          {EXPENSE_LINES.filter(([key]) => breakdown[key] !== 0).map(([key, label]) => (
            <FinanceLine key={key} label={label} cents={-breakdown[key]} />
          ))}
          <span>Expenses</span>
          <strong>{money(-summary.expenses_cents)}</strong>
        </div>
      </Drill>
      <Drill label="Admissions" value={String(summary.admissions)}>
        <p>
          {summary.admissions} guests × {money(summary.admission_price_cents)} ={" "}
          {money(summary.admissions_income_cents)} today; {snapshot.guest_count} in the park now.
        </p>
      </Drill>
      <Drill
        label="Animal welfare"
        value={summary.welfare === null ? "No animals" : `${summary.welfare}`}
        empty={summary.welfare_habitats.length === 0}
      >
        <ul className="drill-list">
          {summary.welfare_habitats.map((entry) => (
            <li key={entry.habitat_id}>
              <button
                onClick={() =>
                  onJump({ kind: "habitat", id: entry.habitat_id }, center(entry.habitat_id))
                }
              >
                Habitat #{entry.habitat_id} · {entry.animals} {entry.species ?? "animals"}
              </button>
              <strong>{entry.welfare}</strong>
            </li>
          ))}
        </ul>
      </Drill>
      <Drill label="Complaints" value={String(summary.complaints)} empty={summary.complaints === 0}>
        <ul className="drill-list">
          {summary.complaint_groups
            .filter((group) => group.guest_ids.length > 0)
            .map((group) => (
              <li key={group.category}>
                <span>
                  {COMPLAINT_LABELS[group.category]} · {group.guest_ids.length}
                </span>
                <span className="drill-links">
                  {group.guest_ids.slice(0, 6).map((id) => {
                    const entry = guest(id)
                    return entry ? (
                      <button key={id} onClick={() => onJump({ kind: "guest", id }, entry)}>
                        #{id}
                      </button>
                    ) : null
                  })}
                </span>
              </li>
            ))}
        </ul>
      </Drill>
      <Drill
        label="Unfinished work"
        value={String(summary.unfinished)}
        empty={summary.unfinished === 0}
      >
        <ul className="drill-list">
          {summary.open_jobs.map((job) => (
            <li key={job.job_id}>
              <button
                onClick={() =>
                  onJump({ kind: "habitat", id: job.habitat_id }, center(job.habitat_id))
                }
              >
                {job.category} · habitat #{job.habitat_id}
              </button>
              <span>{job.state === "due" ? "due" : "in progress"}</span>
            </li>
          ))}
          {summary.litter > 0 && (
            <li>
              <button
                onClick={() => onJump({ kind: "paths" }, snapshot.litter[0] ?? snapshot.entrance)}
              >
                Litter on paths
              </button>
              <span>{summary.litter}</span>
            </li>
          )}
          {summary.maintenance > 0 && (
            <li>
              <span>Stand maintenance</span>
              <span>{summary.maintenance}</span>
            </li>
          )}
        </ul>
      </Drill>
    </div>
  )
}

function FinanceLine({ label, cents }: { label: string; cents: number }) {
  return (
    <>
      <span>{label}</span>
      <strong>{money(cents)}</strong>
    </>
  )
}

type Column<Row> = {
  key: string
  label: string
  /** The sort and search value. */
  value: (row: Row) => string | number
  render?: (row: Row) => ReactNode
  numeric?: boolean
}

export function DataTable<Row>({
  label,
  rows,
  columns,
  rowKey,
  onRow,
  empty,
}: {
  label: string
  rows: Row[]
  columns: Column<Row>[]
  rowKey: (row: Row) => string | number
  onRow?: (row: Row) => void
  empty: string
}) {
  const [query, setQuery] = useState("")
  const [sort, setSort] = useState<{ key: string; descending: boolean } | null>(null)
  const searchId = useId()
  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const filtered = needle
      ? rows.filter((row) =>
          columns.some((column) => String(column.value(row)).toLowerCase().includes(needle)),
        )
      : rows
    const column = sort && columns.find((entry) => entry.key === sort.key)
    if (!sort || !column) {
      return filtered
    }
    return [...filtered].sort((left, right) => {
      const a = column.value(left)
      const b = column.value(right)
      const order =
        typeof a === "number" && typeof b === "number" ? a - b : String(a).localeCompare(String(b))
      return sort.descending ? -order : order
    })
  }, [columns, query, rows, sort])
  return (
    <div className="data-table">
      <label className="table-search" htmlFor={searchId}>
        <span>Search</span>
        <input
          id={searchId}
          type="search"
          value={query}
          placeholder={`Filter ${label.toLowerCase()}`}
          onChange={(event) => setQuery(event.target.value)}
        />
      </label>
      {shown.length === 0 ? (
        <p className="empty-state" role="status">
          {rows.length === 0 ? empty : `No ${label.toLowerCase()} match “${query}”.`}
        </p>
      ) : (
        <div className="table-scroll">
          <table aria-label={label}>
            <thead>
              <tr>
                {columns.map((column) => {
                  const active = sort?.key === column.key
                  let direction: "ascending" | "descending" | "none" = "none"
                  if (active) {
                    direction = sort.descending ? "descending" : "ascending"
                  }
                  const arrow = { ascending: " ▴", descending: " ▾", none: "" }[direction]
                  return (
                    <th
                      key={column.key}
                      aria-sort={direction}
                      className={column.numeric ? "numeric" : ""}
                    >
                      <button
                        onClick={() =>
                          setSort(
                            active
                              ? { key: column.key, descending: !sort.descending }
                              : { key: column.key, descending: Boolean(column.numeric) },
                          )
                        }
                      >
                        {column.label}
                        <span aria-hidden="true">{arrow}</span>
                      </button>
                    </th>
                  )
                })}
              </tr>
            </thead>
            <tbody>
              {shown.map((row) => (
                <tr
                  key={rowKey(row)}
                  tabIndex={onRow ? 0 : undefined}
                  onClick={onRow ? () => onRow(row) : undefined}
                  onKeyDown={
                    onRow
                      ? (event) => {
                          if (event.key === "Enter" || event.key === " ") {
                            event.preventDefault()
                            onRow(row)
                          }
                        }
                      : undefined
                  }
                >
                  {columns.map((column) => (
                    <td key={column.key} className={column.numeric ? "numeric" : ""}>
                      {column.render ? column.render(row) : column.value(row)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

type StaffRow = {
  key: string
  role: "Keeper" | "Janitor" | "Mechanic"
  id: number
  status: string
  assignment: string
  work: number
  at: Point
  target: JumpTarget
}

type TaskRow = {
  key: string
  task: string
  where: string
  state: string
  waiting: string
  age: number
  at: Point
  target: JumpTarget
}

export function ManagementWorkspace({
  snapshot,
  tab,
  onTab,
  onClose,
  onJump,
  onAction,
  actionLabel,
}: {
  snapshot: Snapshot
  tab: WorkspaceTab
  onTab: (tab: WorkspaceTab) => void
  onClose: () => void
  onJump: (target: JumpTarget, focus: Point) => void
  onAction: (alert: Alert) => void
  actionLabel: (alert: Alert) => string | null
}) {
  const label = (key: string | null) =>
    key === null
      ? "Empty"
      : (snapshot.species_catalog.find((offer) => offer.key === key)?.label ?? key)
  const habitatCenter = (habitat: Snapshot["habitats"][number]): Point => ({
    x: habitat.x + Math.floor(habitat.width / 2),
    y: habitat.y + Math.floor(habitat.height / 2),
  })
  const depot = snapshot.animal_care_depot
  const staff: StaffRow[] = [
    ...depot.keepers.map((keeper) => ({
      key: `keeper:${keeper.id}`,
      role: "Keeper" as const,
      id: keeper.id,
      status: keeper.status,
      assignment: keeper.zone_id === null ? "Needs a zone" : `Zone #${keeper.zone_id}`,
      work: keeper.deliveries_completed,
      at: keeper,
      target: { kind: "keeper" as const, id: keeper.id },
    })),
    ...depot.janitors.map((janitor) => ({
      key: `janitor:${janitor.id}`,
      role: "Janitor" as const,
      id: janitor.id,
      status: janitor.status,
      assignment:
        janitor.target_litter_id === null ? "Idle" : `Litter #${janitor.target_litter_id}`,
      work: janitor.tasks_completed,
      at: janitor,
      target: { kind: "depot" as const },
    })),
    ...depot.mechanics.map((mechanic) => ({
      key: `mechanic:${mechanic.id}`,
      role: "Mechanic" as const,
      id: mechanic.id,
      status: mechanic.status,
      assignment:
        mechanic.target_maintenance_id === null
          ? "Idle"
          : `Repair #${mechanic.target_maintenance_id}`,
      work: mechanic.repairs_completed,
      at: mechanic,
      target: { kind: "depot" as const },
    })),
  ]
  const tasks: TaskRow[] = [
    ...snapshot.habitats.flatMap((habitat) =>
      habitat.care_jobs
        .filter((job) => job.state === "due" || job.state === "in_progress")
        .map((job) => ({
          key: `job:${job.id}`,
          task: `${job.category} job #${job.id}`,
          where: `Habitat #${habitat.id}`,
          state: job.state === "due" ? "Due" : "In progress",
          waiting: job.waiting_reason ?? "",
          age: job.expires_in_minutes,
          at: habitatCenter(habitat),
          target: { kind: "habitat" as const, id: habitat.id },
        })),
    ),
    ...snapshot.litter.map((task) => ({
      key: `litter:${task.id}`,
      task: `Litter #${task.id}`,
      where: `Path ${task.x},${task.y}`,
      state: task.assigned_janitor_id === null ? "Waiting" : `Janitor #${task.assigned_janitor_id}`,
      waiting: task.status,
      age: task.age_minutes,
      at: task,
      target: { kind: "paths" as const },
    })),
    ...snapshot.maintenance.map((task) => ({
      key: `maintenance:${task.id}`,
      task: `Repair #${task.id}`,
      where: `Stand #${task.concession_id}`,
      state:
        task.assigned_mechanic_id === null ? "Waiting" : `Mechanic #${task.assigned_mechanic_id}`,
      waiting: task.status,
      age: task.age_minutes,
      at: task,
      target: { kind: "concession" as const, id: task.concession_id },
    })),
  ]
  const facilityLabel = (kind: string) =>
    snapshot.facility_catalog.find((offer) => offer.key === kind)?.label ?? kind
  const ledger = (day: Snapshot["finance"]["current_day"] | null, key: keyof FinanceBreakdown) =>
    day ? day.breakdown[key] : 0
  const financeRows = [
    ...INCOME_LINES.map(([key, name]) => ({ key, name, kind: "Income", sign: 1 })),
    ...EXPENSE_LINES.map(([key, name]) => ({ key, name, kind: "Expense", sign: -1 })),
  ].map((line) => ({
    ...line,
    today: line.sign * ledger(snapshot.finance.current_day, line.key),
    previous:
      snapshot.finance.previous_day === null
        ? null
        : line.sign * ledger(snapshot.finance.previous_day, line.key),
  }))

  return (
    <section className="management-workspace bevel" aria-label="Management workspace">
      <div className="window-title">
        <span>Management</span>
        <button onClick={onClose} aria-label="Close management workspace">
          ×
        </button>
      </div>
      <div className="workspace-tabs" role="tablist" aria-label="Management views">
        {TABS.map((entry) => (
          <button
            key={entry.tab}
            role="tab"
            aria-selected={tab === entry.tab}
            className={tab === entry.tab ? "active" : ""}
            onClick={() => onTab(entry.tab)}
          >
            {entry.label}
            {entry.tab === "alerts" && snapshot.alerts.length > 0 && (
              <span className="tab-count"> {snapshot.alerts.length}</span>
            )}
          </button>
        ))}
      </div>
      <div className="workspace-body" role="tabpanel">
        {tab === "alerts" && (
          <AlertList
            alerts={snapshot.alerts}
            onJump={onJump}
            onAction={onAction}
            actionLabel={actionLabel}
          />
        )}
        {tab === "summary" && <DailySummaryView snapshot={snapshot} onJump={onJump} />}
        {tab === "habitats" && (
          <DataTable
            label="Habitats"
            rows={snapshot.habitats}
            rowKey={(habitat) => habitat.id}
            onRow={(habitat) => onJump({ kind: "habitat", id: habitat.id }, habitatCenter(habitat))}
            empty="No habitats yet: choose Draw habitat and fence one beside a path."
            columns={[
              { key: "id", label: "#", value: (habitat) => habitat.id, numeric: true },
              {
                key: "species",
                label: "Animals",
                value: (habitat) =>
                  habitat.animals === 0 ? "Empty" : `${habitat.animals} ${label(habitat.species)}`,
              },
              {
                key: "welfare",
                label: "Welfare",
                value: (habitat) => (habitat.animals === 0 ? -1 : habitat.welfare),
                render: (habitat) => (habitat.animals === 0 ? "–" : habitat.welfare),
                numeric: true,
              },
              { key: "care", label: "Care", value: (habitat) => habitat.care_status },
              {
                key: "jobs",
                label: "Open jobs",
                value: (habitat) =>
                  habitat.care_jobs.filter(
                    (job) => job.state === "due" || job.state === "in_progress",
                  ).length,
                numeric: true,
              },
              {
                key: "viewing",
                label: "Viewers",
                value: (habitat) => habitat.viewing_occupancy,
                render: (habitat) => `${habitat.viewing_occupancy}/${habitat.viewing_capacity}`,
                numeric: true,
              },
            ]}
          />
        )}
        {tab === "guests" && (
          <DataTable
            label="Guests"
            rows={snapshot.guests}
            rowKey={(guest) => guest.id}
            onRow={(guest) => onJump({ kind: "guest", id: guest.id }, guest)}
            empty="No guests yet: guests come once a stocked habitat can be seen from the entrance path."
            columns={[
              { key: "id", label: "#", value: (guest) => guest.id, numeric: true },
              { key: "thought", label: "Thinking", value: (guest) => guest.thought },
              { key: "happy", label: "Happy", value: (guest) => guest.happiness, numeric: true },
              { key: "hunger", label: "Hunger", value: (guest) => guest.hunger, numeric: true },
              { key: "thirst", label: "Thirst", value: (guest) => guest.thirst, numeric: true },
              { key: "energy", label: "Energy", value: (guest) => guest.energy, numeric: true },
            ]}
          />
        )}
        {tab === "staff" && (
          <>
            <DataTable
              label="Staff"
              rows={staff}
              rowKey={(row) => row.key}
              onRow={(row) => onJump(row.target, row.at)}
              empty="No staff yet: hire keepers, janitors and mechanics at the depot."
              columns={[
                { key: "role", label: "Role", value: (row) => `${row.role} #${row.id}` },
                { key: "assignment", label: "Assigned", value: (row) => row.assignment },
                { key: "status", label: "Doing", value: (row) => row.status },
                { key: "work", label: "Done", value: (row) => row.work, numeric: true },
              ]}
            />
            <h3>Open tasks</h3>
            <DataTable
              label="Tasks"
              rows={tasks}
              rowKey={(row) => row.key}
              onRow={(row) => onJump(row.target, row.at)}
              empty="No open tasks: care jobs, litter and repairs are all done."
              columns={[
                { key: "task", label: "Task", value: (row) => row.task },
                { key: "where", label: "Where", value: (row) => row.where },
                { key: "state", label: "State", value: (row) => row.state },
                { key: "waiting", label: "Note", value: (row) => row.waiting },
              ]}
            />
          </>
        )}
        {tab === "facilities" && (
          <DataTable
            label="Facilities"
            rows={snapshot.concessions}
            rowKey={(stand) => stand.id}
            onRow={(stand) => onJump({ kind: "concession", id: stand.id }, stand)}
            empty="No stands yet: place food and drink stands beside busy paths."
            columns={[
              {
                key: "stand",
                label: "Stand",
                value: (stand) => `${facilityLabel(stand.kind)} #${stand.id}`,
              },
              {
                key: "condition",
                label: "Condition",
                value: (stand) => stand.condition,
                render: (stand) => `${stand.condition}% · ${stand.service_state}`,
                numeric: true,
              },
              {
                key: "sales",
                label: "Sales today",
                value: (stand) => stand.sales_today,
                numeric: true,
              },
              {
                key: "revenue",
                label: "Revenue",
                value: (stand) => stand.total_revenue_cents,
                render: (stand) => money(stand.total_revenue_cents),
                numeric: true,
              },
              {
                key: "maintenance",
                label: "Maintenance",
                value: (stand) => stand.maintenance_status,
              },
            ]}
          />
        )}
        {tab === "finances" && (
          <DataTable
            label="Ledger"
            rows={financeRows}
            rowKey={(row) => row.key}
            empty="No ledger lines."
            columns={[
              { key: "name", label: "Line", value: (row) => row.name },
              { key: "kind", label: "Kind", value: (row) => row.kind },
              {
                key: "today",
                label: `Day ${snapshot.day}`,
                value: (row) => row.today,
                render: (row) => money(row.today),
                numeric: true,
              },
              {
                key: "previous",
                label: "Previous day",
                value: (row) => row.previous ?? 0,
                render: (row) => (row.previous === null ? "–" : money(row.previous)),
                numeric: true,
              },
            ]}
          />
        )}
      </div>
    </section>
  )
}

/** The active overlay's legend and its problem list; each problem jumps to its object. */
export function OverlayLegend({
  view,
  onJump,
  onClose,
}: {
  view: OverlayView
  onJump: (target: JumpTarget, focus: Point) => void
  onClose: () => void
}) {
  return (
    <section className="overlay-legend bevel" aria-label={`${view.title} overlay`}>
      <div className="window-title">
        <span>{view.title} overlay</span>
        <button onClick={onClose} aria-label="Hide overlay">
          ×
        </button>
      </div>
      <ul className="legend-levels">
        {view.legend.map((entry) => (
          <li key={entry.level}>
            <span
              className={`legend-swatch ${entry.level}`}
              style={{ background: OVERLAY_COLORS[entry.level] }}
              aria-hidden="true"
            >
              {LEVEL_ICON[entry.level]}
            </span>
            {entry.label}
            <strong>{view[entry.level]}</strong>
          </li>
        ))}
      </ul>
      {view.items.length === 0 ? (
        <p className="empty-state" role="status">
          {view.cells.length === 0 ? "Nothing to grade yet." : "No problems on this overlay."}
        </p>
      ) : (
        <ul className="overlay-items">
          {view.items.slice(0, 8).map((item, index) => (
            <li key={`${item.focus.x}:${item.focus.y}:${index}`} className={item.level}>
              <button onClick={() => onJump(item.subject, item.focus)}>
                <span aria-hidden="true">{LEVEL_ICON[item.level]}</span> {item.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
