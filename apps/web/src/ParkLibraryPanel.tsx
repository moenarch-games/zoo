import { useEffect, useRef, useState } from "react"

import type { ScenarioSummary } from "./game-types"
import {
  AUTOSAVE_CADENCES,
  type AutosaveCadence,
  background,
  describeSave,
  type SaveSummary,
  type SlotEntry,
} from "./park-storage"
import type { ParkLibraryState } from "./use-park-library"

export type PanelPrompt = {
  message: string
  actions: { label: string; run: () => void }[]
}

type Props = {
  mode: "start" | "menu"
  library: ParkLibraryState
  message: string
  messageKind: "info" | "error"
  prompt: PanelPrompt | null
  onContinue: () => void
  scenarios: ScenarioSummary[]
  /** The view the panel opens on; the park manager opens the New park picker directly. */
  initialView: "main" | "new"
  onNew: (scenarioId: string) => void
  onLoad: (entry: SlotEntry, save: SaveSummary) => void
  onImport: (file: File) => void
  onSaveAs: (name: string) => void
  onClose: () => void
}

type View = "main" | "new" | "load" | "settings" | "save-as"

const VIEW_TITLES: Record<View, string> = {
  main: "",
  new: " · New park",
  load: " · Load park",
  settings: " · Settings",
  "save-as": " · Save as",
}

function savedAt(time: number) {
  return new Date(time).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
}

/**
 * The start screen and the in-game park menu (#89): continue, new park, load, import,
 * settings, and, during play, save, save as, and export.
 */
export function ParkLibraryPanel({
  mode,
  library,
  message,
  messageKind,
  prompt,
  onContinue,
  scenarios,
  initialView,
  onNew,
  onLoad,
  onImport,
  onSaveAs,
  onClose,
}: Props) {
  const [view, setView] = useState<View>(initialView)
  const [scenarioId, setScenarioId] = useState(scenarios[0]?.id ?? "")
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)
  const [saveAsName, setSaveAsName] = useState("")
  const [expanded, setExpanded] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)
  const panelRef = useRef<HTMLElement>(null)
  const { entries, active, conflict } = library
  const latest = entries[0]

  useEffect(() => {
    panelRef.current
      ?.querySelector<HTMLElement>(
        ".library-body button:not(:disabled), .library-body input, .library-body select",
      )
      ?.focus()
  }, [view, prompt])

  useEffect(() => {
    if (mode !== "menu") {
      return
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (view === "main") {
          onClose()
        } else {
          setView("main")
        }
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [mode, onClose, view])

  const back = (
    <button className="secondary" onClick={() => setView("main")}>
      Back
    </button>
  )

  let body
  if (prompt) {
    body = (
      <div className="library-prompt" role="alertdialog" aria-label="Confirm">
        <p>{prompt.message}</p>
        <div className="library-actions">
          {prompt.actions.map((action) => (
            <button key={action.label} onClick={action.run}>
              {action.label}
            </button>
          ))}
        </div>
      </div>
    )
  } else if (view === "new") {
    const chosen = scenarios.find((scenario) => scenario.id === scenarioId) ?? scenarios[0]
    body = (
      <>
        <div className="scenario-list" role="radiogroup" aria-label="Starting park">
          {scenarios.map((scenario) => (
            <button
              key={scenario.id}
              role="radio"
              aria-checked={scenario.id === chosen?.id}
              className={`scenario-card${scenario.id === chosen?.id ? " chosen" : ""}`}
              data-scenario={scenario.id}
              onClick={() => setScenarioId(scenario.id)}
            >
              <strong>{scenario.title}</strong>
              <small>
                {scenario.width}×{scenario.height} tiles · $
                {(scenario.starting_cash_cents / 100).toLocaleString()}
                {scenario.keeper_training ? " · Keeper training" : ""}
              </small>
              <span>{scenario.summary}</span>
              {scenario.objectives.length > 0 && (
                <small>Goals: {scenario.objectives.join(" · ")}</small>
              )}
            </button>
          ))}
        </div>
        <div className="library-actions">
          <button disabled={!chosen} onClick={() => chosen && onNew(chosen.id)}>
            {chosen ? `Start “${chosen.title}”` : "Start park"}
          </button>
          {back}
        </div>
      </>
    )
  } else if (view === "load") {
    body = (
      <>
        {entries.length === 0 && <p>No saved parks in this browser yet.</p>}
        <ul className="library-slots" aria-label="Saved parks">
          {entries.map((entry) => {
            const { slot } = entry
            const newest = entry.saves[0]
            return (
              <li
                key={slot.id}
                className={slot.id === active?.id ? "active" : ""}
                data-slot={slot.name}
              >
                {renaming?.id === slot.id ? (
                  <form
                    className="library-rename"
                    onSubmit={(event) => {
                      event.preventDefault()
                      const name = renaming.name.trim()
                      if (name) {
                        background(library.rename(slot.id, name))
                      }
                      setRenaming(null)
                    }}
                  >
                    <input
                      aria-label="Park name"
                      value={renaming.name}
                      maxLength={60}
                      onChange={(event) => setRenaming({ id: slot.id, name: event.target.value })}
                      autoFocus
                    />
                    <button type="submit">Rename</button>
                    <button type="button" className="secondary" onClick={() => setRenaming(null)}>
                      Cancel
                    </button>
                  </form>
                ) : (
                  <div className="library-slot-head">
                    <strong>{slot.name}</strong>
                    <small>
                      {slot.scenario}
                      {newest ? ` · ${describeSave(newest)}` : ""}
                    </small>
                    <small>
                      {newest
                        ? `${newest.kind === "auto" ? "Autosaved" : "Saved"} ${savedAt(newest.savedAt)}`
                        : "No save"}
                    </small>
                  </div>
                )}
                {confirmDelete === slot.id ? (
                  <div className="library-prompt" role="alertdialog" aria-label="Confirm delete">
                    <p>
                      Delete “{slot.name}” and all of its saves from this browser? This cannot be
                      undone.
                    </p>
                    <div className="library-actions">
                      <button
                        className="danger"
                        onClick={() => {
                          setConfirmDelete(null)
                          background(library.remove(slot.id))
                        }}
                      >
                        Delete park
                      </button>
                      <button className="secondary" onClick={() => setConfirmDelete(null)}>
                        Keep park
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="library-actions">
                    <button disabled={!newest} onClick={() => newest && onLoad(entry, newest)}>
                      Load
                    </button>
                    <button
                      className="secondary"
                      onClick={() => setRenaming({ id: slot.id, name: slot.name })}
                    >
                      Rename
                    </button>
                    <button
                      className="secondary"
                      disabled={!newest}
                      onClick={() => void library.exportSlot(entry)}
                    >
                      Export
                    </button>
                    <button className="secondary" onClick={() => setConfirmDelete(slot.id)}>
                      Delete
                    </button>
                    {entry.saves.length > 1 && (
                      <button
                        className="secondary"
                        aria-expanded={expanded === slot.id}
                        onClick={() => setExpanded(expanded === slot.id ? null : slot.id)}
                      >
                        Earlier saves
                      </button>
                    )}
                  </div>
                )}
                {expanded === slot.id && (
                  <ul className="library-saves" aria-label={`Saves of ${slot.name}`}>
                    {entry.saves.map((save) => (
                      <li key={save.key}>
                        <span>
                          {save.kind === "auto" ? "Autosave" : "Manual save"} · {describeSave(save)}{" "}
                          · {savedAt(save.savedAt)}
                        </span>
                        <button className="secondary" onClick={() => onLoad(entry, save)}>
                          Load this
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            )
          })}
        </ul>
        <div className="library-actions">{back}</div>
      </>
    )
  } else if (view === "settings") {
    body = (
      <>
        <label className="library-field">
          Autosave
          <select
            value={library.cadence}
            onChange={(event) => library.setCadence(Number(event.target.value) as AutosaveCadence)}
          >
            {AUTOSAVE_CADENCES.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <p className="library-note">
          Autosaves keep the three most recent snapshots of each park, and also run when the tab is
          hidden. Parks are stored only in this browser; export them to move them elsewhere.
        </p>
        <div className="library-actions">{back}</div>
      </>
    )
  } else if (view === "save-as") {
    body = (
      <form
        className="library-rename"
        onSubmit={(event) => {
          event.preventDefault()
          const name = saveAsName.trim()
          if (name) {
            onSaveAs(name)
            setView("main")
          }
        }}
      >
        <input
          aria-label="New park name"
          value={saveAsName}
          maxLength={60}
          onChange={(event) => setSaveAsName(event.target.value)}
        />
        <button type="submit">Save as new park</button>
        <button type="button" className="secondary" onClick={() => setView("main")}>
          Cancel
        </button>
      </form>
    )
  } else {
    body = (
      <>
        {conflict && mode === "menu" && (
          <div className="library-prompt" role="alert">
            <p>{conflict.message} Autosave is paused so nothing is overwritten.</p>
            <div className="library-actions">
              <button
                onClick={() => {
                  setSaveAsName(`${active?.name ?? "Park"} (this tab)`)
                  setView("save-as")
                }}
              >
                Save this tab as a new park
              </button>
              <button className="secondary" onClick={() => void library.save("manual", true)}>
                Overwrite with this tab
              </button>
              <button className="secondary" onClick={() => setView("load")}>
                Open the newer save
              </button>
            </div>
          </div>
        )}
        <div className="library-menu">
          {mode === "start" && (
            <button disabled={!latest?.saves[0]} onClick={onContinue}>
              {latest?.saves[0] ? `Continue “${latest.slot.name}”` : "Continue"}
              {latest?.saves[0] && <small>{describeSave(latest.saves[0])}</small>}
            </button>
          )}
          {mode === "menu" && (
            <>
              <button disabled={!library.available} onClick={() => void library.save("manual")}>
                Save
                <small>{library.status}</small>
              </button>
              <button
                disabled={!library.available}
                onClick={() => {
                  setSaveAsName(`${active?.name ?? "Park"} copy`)
                  setView("save-as")
                }}
              >
                Save as…
              </button>
              <button onClick={library.exportCurrent}>Export park file</button>
            </>
          )}
          <button onClick={() => setView("new")}>New park</button>
          <button disabled={!library.available} onClick={() => setView("load")}>
            Load park…
          </button>
          <button onClick={() => fileRef.current?.click()}>Import park file…</button>
          <button onClick={() => setView("settings")}>Settings</button>
          {mode === "menu" && (
            <button className="secondary" onClick={onClose}>
              Back to park
            </button>
          )}
        </div>
      </>
    )
  }

  return (
    <section
      ref={panelRef}
      className={`library-panel bevel ${mode === "start" ? "library-start" : "library-dialog"}`}
      role="dialog"
      aria-modal={mode === "menu"}
      aria-label={mode === "start" ? "Zoo start" : "Park menu"}
    >
      <div className="window-title">
        <span>
          {mode === "start" ? "Zoo" : (active?.name ?? "Unsaved park")}
          {VIEW_TITLES[view]}
        </span>
        {mode === "menu" && (
          <button onClick={onClose} aria-label="Close park menu">
            ×
          </button>
        )}
      </div>
      <div className="library-body">
        {library.storageNotice && (
          <p className="library-note error" role="alert">
            {library.storageNotice}
          </p>
        )}
        {body}
        <p className={`library-message${messageKind === "error" ? " error" : ""}`} role="status">
          {message}
        </p>
        {library.exportFallback && (
          <>
            <p role="alert">The browser blocked the download. Select and copy the save below.</p>
            <textarea
              aria-label="Park save"
              readOnly
              value={library.exportFallback.text}
              rows={4}
            />
            <button className="secondary" onClick={library.dismissExportFallback}>
              Done
            </button>
          </>
        )}
      </div>
      <input
        ref={fileRef}
        type="file"
        accept=".json,application/json"
        hidden
        aria-label="Park save file"
        onChange={(event) => {
          const file = event.target.files?.[0]
          event.target.value = ""
          if (file) {
            onImport(file)
          }
        }}
      />
    </section>
  )
}
