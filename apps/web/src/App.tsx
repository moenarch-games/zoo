import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react"

import { diagnostics, faultInjectionEnabled, installDiagnosticsHook } from "./diagnostics"
import type {
  ActionResult,
  Guest,
  PlacementEvaluation,
  Point,
  Snapshot,
  SpeciesKey,
  SpeciesOffer,
  Speed,
  Tool,
} from "./game-types"
import type { SceneOverlay } from "./park-scene"
import type { ParkPick } from "./ParkView"
import init, { ZooGame } from "./wasm/zoo_core"

// The 3D park view (three.js renderer + zoo-scene WASM) is its own chunk so the HUD and the
// simulation can start without it. The request starts at module load, in parallel with the
// zoo-core WASM, rather than waiting for the first render.
const parkViewModule = import("./ParkView")
const ParkView = lazy(() => parkViewModule)

function money(cents: number) {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 0,
  }).format(cents / 100)
}

function clock(minutes: number) {
  const hours = Math.floor(minutes / 60)
  const mins = minutes % 60
  return `${hours.toString().padStart(2, "0")}:${mins.toString().padStart(2, "0")}`
}

function speciesGlyph(species: SpeciesKey) {
  switch (species) {
    case "flamingo":
      return "🦩"
    case "zebra":
      return "🦓"
    case "giraffe":
      return "🦒"
    case "elephant":
      return "🐘"
    case "penguin":
      return "🐧"
    case "capybara":
      return "C"
  }
}

function speciesLabel(species: SpeciesKey | null, catalog: SpeciesOffer[]) {
  if (!species) {
    return "Empty habitat"
  }
  return catalog.find((offer) => offer.key === species)?.label ?? species
}

function guestStateLabel(state: Guest["state"]) {
  switch (state) {
    case "arriving":
      return "Entering through the gate"
    case "walking_to_habitat":
      return "Walking to habitat"
    case "viewing":
      return "Viewing animals"
    case "walking_to_exit":
      return "Walking to exit"
  }
}

function toolHint(tool: Tool, snapshot: Snapshot | null) {
  if (tool.startsWith("facility:")) {
    const offer = snapshot?.facility_catalog.find((entry) => entry.key === tool.slice(9))
    return offer
      ? `Click clear grass beside a path to build ${offer.label} · ${money(offer.build_cost_cents)}.`
      : "Select an available facility."
  }
  switch (tool) {
    case "path":
      return "Drag across tiles to paint paths · $10 per new tile."
    case "habitat":
      return "Press on one corner, drag to the opposite corner, and release to close the fence."
    case "bulldoze":
      return "Click a path or any tile inside a habitat to remove it."
    case "select":
      return "Click to inspect · drag to pan · right-drag or two-finger twist to rotate · scroll or pinch to zoom."
  }
}

function previewTiles(start: Point | null, end: Point | null, snapshot: Snapshot) {
  if (!start || !end) {
    return []
  }
  const left = Math.min(start.x, end.x)
  const right = Math.max(start.x, end.x)
  const top = Math.min(start.y, end.y)
  const bottom = Math.max(start.y, end.y)
  const tiles: Point[] = []
  for (let y = top; y <= bottom; y += 1) {
    for (let x = left; x <= right; x += 1) {
      if (x >= 0 && y >= 0 && x < snapshot.width && y < snapshot.height) {
        tiles.push({ x, y })
      }
    }
  }
  return tiles
}

type RecoveryReason = "export" | "simulation" | "renderer" | "application"

type RecoveryState = { reason: RecoveryReason; message: string }

const RETAIN_EVERY_TICKS = 8

export default function App() {
  const gameRef = useRef<ZooGame | null>(null)
  // The simulation stops advancing after a WASM trap, a frozen zoo-core session or an
  // uncaught application error; a renderer failure blocks it while its panel is open.
  const stoppedRef = useRef(false)
  const wasmDeadRef = useRef(false)
  const rendererFailedRef = useRef(false)
  const blockedRef = useRef(false)
  const ticksSinceRetainRef = useRef(0)
  const [recovery, setRecovery] = useState<RecoveryState | null>(null)
  const [manualExport, setManualExport] = useState<string | null>(null)
  const paintedTilesRef = useRef(new Set<string>())
  const fenceStartRef = useRef<Point | null>(null)
  const fenceEndRef = useRef<Point | null>(null)

  const [snapshot, setSnapshot] = useState<Snapshot | null>(null)
  const [tool, setTool] = useState<Tool>("select")
  const [speed, setSpeed] = useState<Speed>(1)
  const [message, setMessage] = useState(
    "Extend the entrance path, draw a habitat, then stock and staff it through the care depot.",
  )
  const [messageKind, setMessageKind] = useState<"info" | "error">("info")
  const [selectedHabitatId, setSelectedHabitatId] = useState<number | null>(null)
  const [selectedGuestId, setSelectedGuestId] = useState<number | null>(null)
  const [selectedDepot, setSelectedDepot] = useState(false)
  const [hoveredTile, setHoveredTile] = useState<Point | null>(null)
  const [fenceStart, setFenceStart] = useState<Point | null>(null)
  const [fenceEnd, setFenceEnd] = useState<Point | null>(null)
  const [cameraResetToken, setCameraResetToken] = useState(0)

  const clearFenceGesture = useCallback(() => {
    fenceStartRef.current = null
    fenceEndRef.current = null
    setFenceStart(null)
    setFenceEnd(null)
  }, [])

  const openRecovery = useCallback((reason: RecoveryReason, text: string) => {
    setManualExport(null)
    setRecovery((current) =>
      current && current.reason !== "export" ? current : { reason, message: text },
    )
  }, [])

  const failSimulation = useCallback(
    (error: unknown) => {
      if (wasmDeadRef.current) {
        return
      }
      wasmDeadRef.current = true
      stoppedRef.current = true
      diagnostics.connect(null)
      diagnostics.record("wasm", String(error), "frozen")
      console.error("Zoo simulation trapped; the session is frozen", error)
      openRecovery(
        "simulation",
        "The park simulation crashed and has been stopped. Export the diagnostics before starting a new park.",
      )
    },
    [openRecovery],
  )

  /** Every zoo-core call goes through here so a WASM trap freezes the session once. */
  const callGame = useCallback(
    <T,>(call: (game: ZooGame) => T): T | undefined => {
      const game = gameRef.current
      if (!game || wasmDeadRef.current) {
        return undefined
      }
      try {
        return call(game)
      } catch (error) {
        failSimulation(error)
        return undefined
      }
    },
    [failSimulation],
  )

  const refresh = useCallback(() => {
    const text = callGame((game) => game.snapshot_json())
    if (text !== undefined) {
      setSnapshot(JSON.parse(text) as Snapshot)
    }
  }, [callGame])

  const checkFrozen = useCallback(() => {
    if (stoppedRef.current || callGame((game) => game.session_status()) !== "frozen") {
      return
    }
    stoppedRef.current = true
    diagnostics.retain()
    openRecovery(
      "simulation",
      "The park simulation detected corrupt state and froze. Export the diagnostics before starting a new park.",
    )
  }, [callGame, openRecovery])

  const perform = useCallback(
    (command: () => string) => {
      const text = callGame(() => command())
      const result: ActionResult =
        text === undefined
          ? { ok: false, message: "The park simulation is stopped." }
          : (JSON.parse(text) as ActionResult)
      diagnostics.retain()
      setMessage(result.message)
      setMessageKind(result.ok ? "info" : "error")
      refresh()
      checkFrozen()
      return result
    },
    [callGame, checkFrozen, refresh],
  )

  const onRendererFailure = useCallback(
    (text: string) => {
      if (rendererFailedRef.current) {
        return
      }
      rendererFailedRef.current = true
      diagnostics.record("renderer", text, "continue_paused")
      setSpeed(0)
      blockedRef.current = true
      openRecovery(
        "renderer",
        "The 3D view stopped, but the park simulation is intact and paused. Export diagnostics or continue paused.",
      )
    },
    [openRecovery],
  )

  useEffect(() => {
    const onUncaught = (error: unknown) => {
      const text = error instanceof Error ? error.message : String(error)
      if (text.includes("ResizeObserver loop")) {
        return
      }
      if (error instanceof WebAssembly.RuntimeError) {
        failSimulation(error)
        return
      }
      stoppedRef.current = true
      if (diagnostics.record("application", text, "frozen")) {
        openRecovery(
          "application",
          "An unexpected error stopped the park. Export the diagnostics before starting a new park.",
        )
      }
    }
    const onError = (event: ErrorEvent) => onUncaught(event.error ?? event.message)
    const onRejection = (event: PromiseRejectionEvent) => onUncaught(event.reason)
    window.addEventListener("error", onError)
    window.addEventListener("unhandledrejection", onRejection)
    return () => {
      window.removeEventListener("error", onError)
      window.removeEventListener("unhandledrejection", onRejection)
    }
  }, [failSimulation, openRecovery])

  useEffect(() => {
    let cancelled = false
    init()
      .then(async () => {
        if (cancelled) {
          return
        }
        const response = await fetch(new URL("content/catalog.json", document.baseURI))
        if (!response.ok) {
          throw new Error(`catalog: HTTP ${response.status}`)
        }
        const documentText = await response.text()
        if (cancelled) {
          return
        }
        gameRef.current = ZooGame.from_content_json(documentText)
        diagnostics.connect(() => gameRef.current?.diagnostics_json() ?? "null")
        diagnostics.retain()
        installDiagnosticsHook()
        if (faultInjectionEnabled()) {
          window.__zooFaults = {
            simulation: (kind: string) => {
              const result = callGame((game) => game.inject_fault(kind)) ?? ""
              diagnostics.retain()
              return result
            },
          }
        }
        refresh()
      })
      .catch((error: unknown) => {
        console.error("Zoo simulation initialization failed", error)
        if (!cancelled) {
          setMessage(`The park simulation could not load: ${String(error)}`)
          setMessageKind("error")
        }
      })
    return () => {
      cancelled = true
    }
  }, [callGame, refresh])

  useEffect(() => {
    if (!snapshot || speed === 0) {
      return
    }
    const handle = window.setInterval(() => {
      if (stoppedRef.current || blockedRef.current) {
        return
      }
      callGame((game) => game.tick(speed))
      checkFrozen()
      if (stoppedRef.current) {
        return
      }
      ticksSinceRetainRef.current += 1
      if (ticksSinceRetainRef.current >= RETAIN_EVERY_TICKS) {
        ticksSinceRetainRef.current = 0
        diagnostics.retain()
      }
      refresh()
    }, 250)
    return () => window.clearInterval(handle)
  }, [callGame, checkFrozen, refresh, snapshot !== null, speed])

  useEffect(() => {
    paintedTilesRef.current.clear()
    if (tool !== "habitat") {
      clearFenceGesture()
    }
  }, [clearFenceGesture, tool])

  const selectedHabitat = useMemo(
    () => snapshot?.habitats.find((habitat) => habitat.id === selectedHabitatId) ?? null,
    [selectedHabitatId, snapshot],
  )

  const selectedGuest = useMemo(
    () => snapshot?.guests.find((guest) => guest.id === selectedGuestId) ?? null,
    [selectedGuestId, snapshot],
  )

  const placement = useMemo(() => {
    const game = gameRef.current
    if (!game || !snapshot || tool !== "habitat" || !fenceStart || !fenceEnd) {
      return null
    }
    const text = callGame(() =>
      game.evaluate_habitat_rect(fenceStart.x, fenceStart.y, fenceEnd.x, fenceEnd.y),
    )
    return text === undefined ? null : (JSON.parse(text) as PlacementEvaluation)
  }, [callGame, fenceEnd, fenceStart, snapshot, tool])

  const ghostTiles = useMemo(
    () => (snapshot ? previewTiles(fenceStart, fenceEnd, snapshot) : []),
    [fenceEnd, fenceStart, snapshot?.width, snapshot?.height],
  )

  const overlay = useMemo<SceneOverlay>(
    () => ({
      tool,
      hoveredTile,
      ghostTiles,
      placement,
      selectedHabitatId,
      selectedGuestId,
      selectedDepot,
    }),
    [ghostTiles, hoveredTile, placement, selectedDepot, selectedGuestId, selectedHabitatId, tool],
  )

  const paintPath = useCallback(
    (tile: Point) => {
      const game = gameRef.current
      if (!game) {
        return
      }
      const key = `${tile.x}:${tile.y}`
      if (paintedTilesRef.current.has(key)) {
        return
      }
      paintedTilesRef.current.add(key)
      perform(() => game.place_path(tile.x, tile.y))
    },
    [perform],
  )

  const onTileDown = (tile: Point) => {
    if (tool === "path") {
      paintedTilesRef.current.clear()
      paintPath(tile)
    } else if (tool === "habitat") {
      fenceStartRef.current = tile
      fenceEndRef.current = tile
      setFenceStart(tile)
      setFenceEnd(tile)
    }
  }

  const onTileDrag = (tile: Point) => {
    if (tool === "path") {
      paintPath(tile)
    } else if (tool === "habitat" && fenceStartRef.current) {
      fenceEndRef.current = tile
      setFenceEnd(tile)
    }
  }

  const onToolGestureEnd = (commit: boolean) => {
    paintedTilesRef.current.clear()
    if (tool !== "habitat") {
      return
    }
    const game = gameRef.current
    const start = fenceStartRef.current
    const end = fenceEndRef.current
    if (commit && game && start && end) {
      perform(() => game.place_habitat_rect(start.x, start.y, end.x, end.y))
    }
    clearFenceGesture()
  }

  const inform = (text: string) => {
    setMessage(text)
    setMessageKind("info")
  }

  const selectHabitat = (habitatId: number | null) => {
    setSelectedGuestId(null)
    setSelectedDepot(false)
    setSelectedHabitatId(habitatId)
  }

  const selectDepot = () => {
    setSelectedGuestId(null)
    setSelectedHabitatId(null)
    setSelectedDepot(true)
    inform("Central operations depot selected · stock animal feed and hire park staff here.")
  }

  const describeStand = (standId: number) => {
    const stand = snapshot?.concessions.find((candidate) => candidate.id === standId)
    if (!stand) {
      return
    }
    const label =
      snapshot?.facility_catalog.find((offer) => offer.key === stand.kind)?.label ?? stand.kind
    inform(
      `${label} stand #${stand.id} · ${stand.condition}% condition · ${stand.service_state} · ${stand.maintenance_status}`,
    )
  }

  const onPick = (pick: ParkPick) => {
    const game = gameRef.current
    if (!game || !snapshot) {
      return
    }
    const depot = snapshot.animal_care_depot
    const isDepotTile = (tile: Point | null) => tile?.x === depot.x && tile?.y === depot.y

    if (tool === "select") {
      switch (pick.kind) {
        case "guest":
          setSelectedGuestId(pick.id)
          setSelectedHabitatId(null)
          setSelectedDepot(false)
          return
        case "animal":
          selectHabitat(pick.habitatId)
          inform(`Habitat #${pick.habitatId} selected`)
          return
        case "depot":
          selectDepot()
          return
        case "concession":
          selectHabitat(null)
          describeStand(pick.id)
          return
        case "tile": {
          if (isDepotTile(pick.tile)) {
            selectDepot()
            return
          }
          const tile = snapshot.tiles.find(
            (candidate) => candidate.x === pick.tile.x && candidate.y === pick.tile.y,
          )
          selectHabitat(tile?.habitat_id ?? null)
          if (tile?.habitat_id) {
            inform(`Habitat #${tile.habitat_id} selected`)
          } else if (tile?.concession_id) {
            describeStand(tile.concession_id)
          } else {
            inform("Ground selected")
          }
          return
        }
        case "none":
          selectHabitat(null)
          return
      }
    }

    const target = pick.kind === "none" ? null : pick.tile
    if (tool === "bulldoze") {
      if (pick.kind === "depot" || isDepotTile(target)) {
        setMessage("The central animal-care depot cannot be demolished.")
        setMessageKind("error")
        return
      }
      if (!target) {
        return
      }
      const tile = snapshot.tiles.find(
        (candidate) => candidate.x === target.x && candidate.y === target.y,
      )
      perform(() => game.bulldoze(target.x, target.y))
      if (tile?.habitat_id != null && tile.habitat_id === selectedHabitatId) {
        setSelectedHabitatId(null)
      }
      return
    }

    if (tool.startsWith("facility:") && target) {
      perform(() => game.place_concession(target.x, target.y, tool.slice(9)))
    }
  }

  const adopt = (species: SpeciesKey) => {
    const game = gameRef.current
    if (!game || selectedHabitatId === null) {
      return
    }
    perform(() => game.adopt(selectedHabitatId, species))
  }

  const careForHabitat = (action: "water" | "clean" | "shelter") => {
    const game = gameRef.current
    if (!game || selectedHabitatId === null) {
      return
    }
    if (action === "water") {
      perform(() => game.refill_water(selectedHabitatId))
    }
    if (action === "clean") {
      perform(() => game.clean_habitat(selectedHabitatId))
    }
    if (action === "shelter") {
      perform(() => game.add_shelter(selectedHabitatId))
    }
  }

  const scheduleKeeper = () => {
    const game = gameRef.current
    if (!game || selectedHabitatId === null) {
      return
    }
    perform(() => game.schedule_keeper(selectedHabitatId))
  }

  const buyAnimalFeed = () => {
    const game = gameRef.current
    if (!game) {
      return
    }
    perform(() => game.buy_animal_feed())
  }

  const hireKeeper = () => {
    const game = gameRef.current
    if (!game) {
      return
    }
    perform(() => game.hire_keeper())
  }

  const hireJanitor = () => {
    const game = gameRef.current
    if (!game) {
      return
    }
    perform(() => game.hire_janitor())
  }

  const hireMechanic = () => {
    const game = gameRef.current
    if (!game) {
      return
    }
    perform(() => game.hire_mechanic())
  }

  const storageFailure = (action: string, error: unknown, text: string) => {
    diagnostics.record("storage", `${action} diagnostics failed: ${String(error)}`, "manual_export")
    setManualExport(text)
  }

  const downloadDiagnostics = () => {
    const text = diagnostics.exportText()
    try {
      const url = URL.createObjectURL(new Blob([text], { type: "application/json" }))
      const link = document.createElement("a")
      link.href = url
      link.download = "zoo-diagnostics.json"
      document.body.append(link)
      link.click()
      link.remove()
      window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
    } catch (error) {
      storageFailure("Download", error, text)
    }
  }

  const copyDiagnostics = () => {
    const text = diagnostics.exportText()
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("clipboard unavailable")
      }
      navigator.clipboard.writeText(text).then(
        () => inform("Diagnostics copied to the clipboard."),
        (error: unknown) => storageFailure("Copy", error, text),
      )
    } catch (error) {
      storageFailure("Copy", error, text)
    }
  }

  const openExport = () => {
    diagnostics.retain()
    setManualExport(null)
    setRecovery(
      (current) =>
        current ?? {
          reason: "export",
          message: "Save the bounded park diagnostics. Nothing is uploaded.",
        },
    )
  }

  const continuePaused = () => {
    blockedRef.current = false
    setSpeed(0)
    setRecovery(null)
    setManualExport(null)
  }

  const closeExport = () => {
    setRecovery(null)
    setManualExport(null)
  }

  const startNewPark = () => {
    if (wasmDeadRef.current || recovery?.reason === "application") {
      window.location.reload()
      return
    }
    stoppedRef.current = false
    setRecovery(null)
    setManualExport(null)
    reset()
  }

  const reset = () => {
    paintedTilesRef.current.clear()
    clearFenceGesture()
    callGame((game) => game.reset())
    diagnostics.retain()
    setTool("select")
    setSelectedHabitatId(null)
    setSelectedGuestId(null)
    setSelectedDepot(false)
    setHoveredTile(null)
    setCameraResetToken((token) => token + 1)
    setMessage("New park started.")
    setMessageKind("info")
    refresh()
  }

  if (!snapshot) {
    return (
      <main className="loading">
        {messageKind === "error" ? <p role="alert">{message}</p> : "Preparing the park simulation…"}
      </main>
    )
  }

  const tooltip =
    placement && hoveredTile ? (
      <div className={placement.ok ? "valid" : "invalid"}>
        <b>
          {placement.ok ? "✓ Fence closes" : "! Cannot build"} · {placement.width}×
          {placement.height} · {money(placement.cost_cents)}
        </b>
        <small>{placement.message}</small>
      </div>
    ) : undefined

  return (
    <main className="game-shell">
      <header className="topbar bevel">
        <div className="brand">
          <strong>Zoo</strong>
          <span>Opening Day</span>
        </div>
        <div className="stat">
          <span>Cash</span>
          <strong>{money(snapshot.cash_cents)}</strong>
        </div>
        <div className="stat">
          <span>Guests</span>
          <strong>{snapshot.guest_count}</strong>
        </div>
        <div className="stat">
          <span>Rating</span>
          <strong>{snapshot.rating}/999</strong>
        </div>
        <div className="stat">
          <span>Day</span>
          <strong>
            {snapshot.day} · {clock(snapshot.minute_of_day)}
          </strong>
        </div>
        <div className="speed-controls" aria-label="Simulation speed">
          <button
            className="view-reset"
            onClick={() => setCameraResetToken((token) => token + 1)}
            title="Reset view (Home)"
            aria-label="Reset view"
          >
            ⌂
          </button>
          {([0, 1, 2, 4] as const).map((value) => (
            <button
              key={value}
              className={speed === value ? "active" : ""}
              onClick={() => setSpeed(value)}
              title={value === 0 ? "Pause" : `${value}x speed`}
            >
              {value === 0 ? "Ⅱ" : `${value}×`}
            </button>
          ))}
          <button
            className="export-diagnostics"
            onClick={openExport}
            title="Export diagnostics"
            aria-label="Export diagnostics"
          >
            ⇩
          </button>
        </div>
      </header>

      <section className="workspace">
        <Suspense fallback={<div className="park-view park-view-loading">Loading 3D park…</div>}>
          <ParkView
            snapshot={snapshot}
            overlay={overlay}
            paused={speed === 0}
            dragTool={tool === "path" || tool === "habitat"}
            resetToken={cameraResetToken}
            tooltip={tooltip}
            onTileDown={onTileDown}
            onTileDrag={onTileDrag}
            onToolGestureEnd={onToolGestureEnd}
            onHover={setHoveredTile}
            onPick={onPick}
            onRendererFailure={onRendererFailure}
          />
        </Suspense>

        {recovery && (
          <section
            className="recovery-panel bevel"
            role="dialog"
            aria-modal="false"
            aria-label="Park recovery"
          >
            <div className="window-title">
              <span>{recovery.reason === "export" ? "Park diagnostics" : "Park stopped"}</span>
            </div>
            <p>{recovery.message}</p>
            <div className="recovery-actions">
              <button onClick={downloadDiagnostics}>Download diagnostics</button>
              <button onClick={copyDiagnostics}>Copy diagnostics</button>
              {recovery.reason === "renderer" && (
                <button onClick={continuePaused}>Continue paused</button>
              )}
              {(recovery.reason === "simulation" || recovery.reason === "application") && (
                <button onClick={startNewPark}>Start new park</button>
              )}
              {recovery.reason === "export" && <button onClick={closeExport}>Close</button>}
            </div>
            {manualExport !== null && (
              <>
                <p role="alert">
                  Saving was blocked by the browser. Select and copy the diagnostics below.
                </p>
                <textarea aria-label="Diagnostics bundle" readOnly value={manualExport} rows={4} />
              </>
            )}
          </section>
        )}

        <aside className="side-panel bevel">
          {selectedDepot && (
            <>
              <div className="window-title">
                <span>Central operations depot</span>
                <button onClick={() => setSelectedDepot(false)}>×</button>
              </div>
              <div className="manager-card">
                <div className="guest-thought">
                  Animal feed and park staff are dispatched from this central facility.
                </div>
                <dl>
                  <div>
                    <dt>Feed stock</dt>
                    <dd>{snapshot.animal_care_depot.feed_crates} crates</dd>
                  </div>
                  <div>
                    <dt>Keepers</dt>
                    <dd>{snapshot.animal_care_depot.keepers.length}</dd>
                  </div>
                  <div>
                    <dt>Wage / keeper</dt>
                    <dd>{money(snapshot.animal_care_depot.keeper_hourly_wage_cents)}/hr</dd>
                  </div>
                  <div>
                    <dt>Janitors</dt>
                    <dd>{snapshot.animal_care_depot.janitors.length}</dd>
                  </div>
                  <div>
                    <dt>Wage / janitor</dt>
                    <dd>{money(snapshot.animal_care_depot.janitor_hourly_wage_cents)}/hr</dd>
                  </div>
                  <div>
                    <dt>Mechanics</dt>
                    <dd>{snapshot.animal_care_depot.mechanics.length}</dd>
                  </div>
                  <div>
                    <dt>Wage / mechanic</dt>
                    <dd>{money(snapshot.animal_care_depot.mechanic_hourly_wage_cents)}/hr</dd>
                  </div>
                  <div>
                    <dt>Maintenance backlog</dt>
                    <dd>{snapshot.operations.maintenance_backlog}</dd>
                  </div>
                  <div>
                    <dt>Oldest maintenance</dt>
                    <dd>{snapshot.operations.oldest_maintenance_age_minutes} min</dd>
                  </div>
                  <div>
                    <dt>Path cleanliness</dt>
                    <dd>{snapshot.operations.cleanliness}%</dd>
                  </div>
                  <div>
                    <dt>Litter backlog</dt>
                    <dd>{snapshot.operations.litter_backlog}</dd>
                  </div>
                  <div>
                    <dt>Oldest litter</dt>
                    <dd>{snapshot.operations.oldest_litter_age_minutes} min</dd>
                  </div>
                </dl>
                <button className="shop-row" onClick={buyAnimalFeed}>
                  <span>
                    <b>Buy {snapshot.animal_care_depot.feed_batch_crates} feed crates</b>
                    <small>Stock used by scheduled habitat food runs</small>
                  </span>
                  <strong>{money(snapshot.animal_care_depot.feed_batch_cost_cents)}</strong>
                </button>
                <button className="shop-row" onClick={hireKeeper}>
                  <span>
                    <b>Hire keeper</b>
                    <small>One keeper can currently serve one habitat</small>
                  </span>
                  <strong>{money(snapshot.animal_care_depot.keeper_hire_cost_cents)}</strong>
                </button>
                <button className="shop-row" onClick={hireJanitor}>
                  <span>
                    <b>Hire janitor</b>
                    <small>Janitors claim and clean reachable path litter</small>
                  </span>
                  <strong>{money(snapshot.animal_care_depot.janitor_hire_cost_cents)}</strong>
                </button>
                <button className="shop-row" onClick={hireMechanic}>
                  <span>
                    <b>Hire mechanic</b>
                    <small>Mechanics respond to reachable facility maintenance</small>
                  </span>
                  <strong>{money(snapshot.animal_care_depot.mechanic_hire_cost_cents)}</strong>
                </button>
                <h3>Keeper schedule</h3>
                {snapshot.animal_care_depot.keepers.length === 0 ? (
                  <div className="guest-thought">No keepers hired yet.</div>
                ) : (
                  snapshot.animal_care_depot.keepers.map((keeper) => (
                    <dl key={keeper.id}>
                      <div>
                        <dt>Keeper #{keeper.id}</dt>
                        <dd>{keeper.status}</dd>
                      </div>
                      <div>
                        <dt>Food deliveries</dt>
                        <dd>{keeper.deliveries_completed}</dd>
                      </div>
                    </dl>
                  ))
                )}
                <h3>Janitor service</h3>
                {snapshot.animal_care_depot.janitors.length === 0 ? (
                  <div className="guest-thought">
                    No janitors hired. Litter will remain visible until staff can reach it.
                  </div>
                ) : (
                  snapshot.animal_care_depot.janitors.map((janitor) => (
                    <dl key={janitor.id}>
                      <div>
                        <dt>Janitor #{janitor.id}</dt>
                        <dd>{janitor.status}</dd>
                      </div>
                      <div>
                        <dt>Cleanups</dt>
                        <dd>{janitor.tasks_completed}</dd>
                      </div>
                    </dl>
                  ))
                )}
                <h3>Mechanic service</h3>
                {snapshot.animal_care_depot.mechanics.length === 0 ? (
                  <div className="guest-thought">
                    No mechanics hired. Worn stands can degrade and eventually close.
                  </div>
                ) : (
                  snapshot.animal_care_depot.mechanics.map((mechanic) => (
                    <dl key={mechanic.id}>
                      <div>
                        <dt>Mechanic #{mechanic.id}</dt>
                        <dd>{mechanic.status}</dd>
                      </div>
                      <div>
                        <dt>Repairs</dt>
                        <dd>{mechanic.repairs_completed}</dd>
                      </div>
                    </dl>
                  ))
                )}
              </div>
            </>
          )}
          {!selectedDepot && selectedGuest && (
            <>
              <div className="window-title">
                <span>Guest #{selectedGuest.id}</span>
                <button onClick={() => setSelectedGuestId(null)}>×</button>
              </div>
              <div className="guest-card">
                <div className="guest-thought">“{selectedGuest.thought}”</div>
                <dl>
                  <div>
                    <dt>Status</dt>
                    <dd>{guestStateLabel(selectedGuest.state)}</dd>
                  </div>
                  <div>
                    <dt>Destination</dt>
                    <dd>Habitat #{selectedGuest.target_habitat}</dd>
                  </div>
                  <div>
                    <dt>Happiness</dt>
                    <dd>{selectedGuest.happiness}%</dd>
                  </div>
                </dl>
                <h3>Needs</h3>
                <NeedBar label="Energy" value={selectedGuest.energy} />
                <NeedBar label="Hunger" value={selectedGuest.hunger} badWhenHigh />
                <NeedBar label="Thirst" value={selectedGuest.thirst} badWhenHigh />
                <NeedBar label="Value" value={selectedGuest.value_perception} />
              </div>
            </>
          )}
          {!selectedDepot && !selectedGuest && selectedHabitat && (
            <>
              <div className="window-title">
                <span>Habitat #{selectedHabitat.id}</span>
                <button onClick={() => setSelectedHabitatId(null)}>×</button>
              </div>
              <div className="habitat-card">
                <div className="habitat-name">
                  {speciesLabel(selectedHabitat.species, snapshot.species_catalog)}
                </div>
                <div className="guest-thought">{selectedHabitat.welfare_status}</div>
                <dl>
                  <div>
                    <dt>Animals</dt>
                    <dd>
                      {selectedHabitat.animals}/{selectedHabitat.capacity}
                    </dd>
                  </div>
                  <div>
                    <dt>Enclosed area</dt>
                    <dd>{selectedHabitat.footprint_area} tiles</dd>
                  </div>
                  <div>
                    <dt>Fence</dt>
                    <dd>{selectedHabitat.fence_length} sections</dd>
                  </div>
                  <div>
                    <dt>Current welfare</dt>
                    <dd>{selectedHabitat.welfare}%</dd>
                  </div>
                  <div>
                    <dt>Welfare target</dt>
                    <dd>{selectedHabitat.welfare_target}%</dd>
                  </div>
                  <div>
                    <dt>Social fit</dt>
                    <dd>{selectedHabitat.social_score}%</dd>
                  </div>
                  <div>
                    <dt>Space fit</dt>
                    <dd>{selectedHabitat.space_score}%</dd>
                  </div>
                  <div>
                    <dt>Appeal</dt>
                    <dd>{selectedHabitat.appeal}</dd>
                  </div>
                </dl>
                <div className="meter">
                  <span style={{ width: `${selectedHabitat.welfare}%` }} />
                </div>

                <h3>Viewing</h3>
                <dl>
                  <div>
                    <dt>Viewers / capacity</dt>
                    <dd>
                      {selectedHabitat.viewing_occupancy}/{selectedHabitat.viewing_capacity}
                    </dd>
                  </div>
                  <div>
                    <dt>Useful path viewpoints</dt>
                    <dd>
                      {selectedHabitat.viewing_spots.filter((spot) => spot.capacity > 0).length}/
                      {selectedHabitat.viewing_spots.length}
                    </dd>
                  </div>
                  <div>
                    <dt>Crowded viewpoints</dt>
                    <dd>{selectedHabitat.viewing_spots.filter((spot) => spot.crowded).length}</dd>
                  </div>
                </dl>
                {selectedHabitat.animals > 0 && selectedHabitat.viewing_capacity === 0 && (
                  <div className="guest-thought">
                    No animals are visible from the current path frontage.
                  </div>
                )}
                {selectedHabitat.viewing_spots.some((spot) => spot.crowded) && (
                  <div className="guest-thought">
                    One or more path viewpoints are over capacity.
                  </div>
                )}

                <h3>Care</h3>
                <div className="guest-thought">{selectedHabitat.care_status}</div>
                <dl>
                  <div>
                    <dt>Food keeper</dt>
                    <dd>
                      {selectedHabitat.keeper_id === null
                        ? "Not scheduled"
                        : `Keeper #${selectedHabitat.keeper_id}`}
                    </dd>
                  </div>
                  <div>
                    <dt>Next food run</dt>
                    <dd>
                      {selectedHabitat.next_feed_delivery_in_minutes === null
                        ? "Not scheduled"
                        : `${selectedHabitat.next_feed_delivery_in_minutes} min`}
                    </dd>
                  </div>
                </dl>
                <div className="guest-thought">{selectedHabitat.feeding_status}</div>
                {selectedHabitat.keeper_id === null && (
                  <button className="shop-row" onClick={scheduleKeeper}>
                    <span>
                      <b>Schedule available keeper</b>
                      <small>Hire keepers at the central care depot first</small>
                    </span>
                  </button>
                )}
                <NeedBar label="Food" value={selectedHabitat.food} />
                <NeedBar label="Water" value={selectedHabitat.water} />
                <NeedBar label="Cleanliness" value={selectedHabitat.cleanliness} />
                <dl>
                  <div>
                    <dt>Shelter</dt>
                    <dd>{selectedHabitat.has_shelter ? "Installed" : "Missing"}</dd>
                  </div>
                </dl>
                <button className="shop-row" onClick={() => careForHabitat("water")}>
                  <span>
                    <b>Refill water</b>
                    <small>Fill habitat water stores</small>
                  </span>
                </button>
                <button className="shop-row" onClick={() => careForHabitat("clean")}>
                  <span>
                    <b>Clean habitat</b>
                    <small>Restore habitat cleanliness</small>
                  </span>
                </button>
                <button className="shop-row" onClick={() => careForHabitat("shelter")}>
                  <span>
                    <b>Add basic shelter</b>
                    <small>Give animals a protected resting area</small>
                  </span>
                </button>

                <h3>Adopt animal</h3>
                {selectedHabitat.keeper_id === null && (
                  <div className="guest-thought">
                    Schedule a keeper before animals can move into this habitat.
                  </div>
                )}
                {snapshot.species_catalog.map((offer) => {
                  const wrongSpecies =
                    selectedHabitat.species !== null && selectedHabitat.species !== offer.key
                  const full = selectedHabitat.animals >= selectedHabitat.capacity
                  const unstaffed = selectedHabitat.keeper_id === null
                  return (
                    <button
                      className="shop-row species-row"
                      key={offer.key}
                      disabled={wrongSpecies || full || unstaffed}
                      onClick={() => adopt(offer.key)}
                    >
                      <span className="species-offer">
                        <i>{speciesGlyph(offer.asset_ref)}</i>
                        <span>
                          <b>{offer.label}</b>
                          <small>
                            Group {offer.minimum_social_group}+ · {offer.space_per_animal} space
                            each
                          </small>
                        </span>
                      </span>
                      <strong>{money(offer.purchase_cost_cents)}</strong>
                    </button>
                  )
                })}
              </div>
            </>
          )}
          {!selectedDepot && !selectedGuest && !selectedHabitat && (
            <>
              <div className="window-title">
                <span>Park manager</span>
              </div>
              <div className="manager-card">
                <h2>Opening objective</h2>
                <ol>
                  <li>Guests enter through the gate on the west edge.</li>
                  <li>Drag the path tool to extend the entrance route.</li>
                  <li>Choose Habitat and drag a closed rectangular fence around clear grass.</li>
                  <li>Open the operations depot, buy animal feed, and hire a keeper.</li>
                  <li>Hire a janitor so path litter has a visible service response.</li>
                  <li>Hire a mechanic before concession wear turns into closures.</li>
                  <li>Select the enclosure and schedule an available keeper.</li>
                  <li>Adopt animals after the habitat has a food-delivery schedule.</li>
                  <li>Place guest food and drink stands on clear grass beside busy paths.</li>
                  <li>Watch staff respond as guests create animal-care and cleanup work.</li>
                </ol>
                <div className="finance-grid">
                  <span>Income today</span>
                  <strong>{money(snapshot.finance.current_day.income_cents)}</strong>
                  <span>Expenses today</span>
                  <strong>{money(snapshot.finance.current_day.expenses_cents)}</strong>
                  <span>Profit today</span>
                  <strong>{money(snapshot.finance.current_day.profit_cents)}</strong>
                  <span>Admission price</span>
                  <strong>{money(snapshot.finance.admission_price_cents)}</strong>
                  <span>Trend</span>
                  <strong>
                    {snapshot.finance.profit_change_cents === null
                      ? "First day"
                      : `${snapshot.finance.profit_trend} · ${money(
                          snapshot.finance.profit_change_cents,
                        )}`}
                  </strong>
                </div>
                <h3>Park operations</h3>
                <div className="finance-grid">
                  <span>Path cleanliness</span>
                  <strong>{snapshot.operations.cleanliness}%</strong>
                  <span>Litter backlog</span>
                  <strong>{snapshot.operations.litter_backlog}</strong>
                  <span>Oldest wait</span>
                  <strong>{snapshot.operations.oldest_litter_age_minutes} min</strong>
                  <span>Janitors</span>
                  <strong>{snapshot.animal_care_depot.janitors.length}</strong>
                  <span>Maintenance backlog</span>
                  <strong>{snapshot.operations.maintenance_backlog}</strong>
                  <span>Oldest maintenance</span>
                  <strong>{snapshot.operations.oldest_maintenance_age_minutes} min</strong>
                  <span>Degraded stands</span>
                  <strong>{snapshot.operations.degraded_concessions}</strong>
                  <span>Failed stands</span>
                  <strong>{snapshot.operations.failed_concessions}</strong>
                  <span>Mechanics</span>
                  <strong>{snapshot.animal_care_depot.mechanics.length}</strong>
                </div>
                <h3>Income breakdown</h3>
                <div className="finance-grid">
                  <span>Admissions</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.admissions_income_cents)}
                  </strong>
                  <span>Guest concessions</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.concession_income_cents)}
                  </strong>
                </div>
                <h3>Expense breakdown</h3>
                <div className="finance-grid">
                  <span>Construction</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.construction_expense_cents)}
                  </strong>
                  <span>Animal purchases</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.animal_purchase_expense_cents)}
                  </strong>
                  <span>Habitat care</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.habitat_care_expense_cents)}
                  </strong>
                  <span>Animal feed</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.animal_feed_expense_cents)}
                  </strong>
                  <span>Keeper hiring</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.keeper_hiring_expense_cents)}
                  </strong>
                  <span>Janitor hiring</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.janitor_hiring_expense_cents)}
                  </strong>
                  <span>Mechanic hiring</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.mechanic_hiring_expense_cents)}
                  </strong>
                  <span>Maintenance repairs</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.maintenance_repair_expense_cents)}
                  </strong>
                  <span>Park upkeep</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.park_upkeep_expense_cents)}
                  </strong>
                  <span>Keeper wages</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.keeper_wages_expense_cents)}
                  </strong>
                  <span>Janitor wages</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.janitor_wages_expense_cents)}
                  </strong>
                  <span>Mechanic wages</span>
                  <strong>
                    {money(snapshot.finance.current_day.breakdown.mechanic_wages_expense_cents)}
                  </strong>
                </div>
                {snapshot.finance.previous_day !== null && (
                  <div className="guest-thought">
                    Day {snapshot.finance.previous_day.day} profit:{" "}
                    {money(snapshot.finance.previous_day.profit_cents)}
                  </div>
                )}
                <h3>Guest complaints</h3>
                <div className="complaint-grid">
                  <span>Hungry</span>
                  <strong>{snapshot.complaints.hungry}</strong>
                  <span>Thirsty</span>
                  <strong>{snapshot.complaints.thirsty}</strong>
                  <span>Tired</span>
                  <strong>{snapshot.complaints.tired}</strong>
                  <span>Poor value</span>
                  <strong>{snapshot.complaints.poor_value}</strong>
                </div>
                <button className="secondary" onClick={reset}>
                  Start new park
                </button>
              </div>
            </>
          )}
        </aside>
      </section>

      <footer className="bottom-dock">
        <div className="message-stack">
          <div className={`message bevel ${messageKind === "error" ? "error" : ""}`}>{message}</div>
          <div className="tool-hint">{toolHint(tool, snapshot)}</div>
        </div>
        <nav className="toolbar bevel" aria-label="Build tools">
          <ToolButton
            active={tool === "select"}
            icon="↖"
            label="Inspect"
            onClick={() => setTool("select")}
          />
          <ToolButton
            active={tool === "path"}
            icon="▦"
            label="Path · $10"
            onClick={() => setTool("path")}
          />
          <ToolButton
            active={tool === "habitat"}
            icon="⌗"
            label="Draw habitat"
            onClick={() => setTool("habitat")}
          />
          {snapshot?.facility_catalog
            .filter((offer) => offer.service === "food" || offer.service === "drink")
            .map((offer) => (
              <ToolButton
                key={offer.key}
                active={tool === `facility:${offer.key}`}
                icon={offer.service === "food" ? "▰" : "▥"}
                label={`${offer.label} · ${money(offer.build_cost_cents)}`}
                onClick={() => setTool(`facility:${offer.key}`)}
              />
            ))}
          <ToolButton
            active={tool === "bulldoze"}
            icon="⌫"
            label="Demolish"
            onClick={() => setTool("bulldoze")}
          />
        </nav>
      </footer>
    </main>
  )
}

function ToolButton({
  active,
  icon,
  label,
  onClick,
}: {
  active: boolean
  icon: string
  label: string
  onClick: () => void
}) {
  return (
    <button className={`tool ${active ? "active" : ""}`} onClick={onClick}>
      <span>{icon}</span>
      <small>{label}</small>
    </button>
  )
}

function NeedBar({
  label,
  value,
  badWhenHigh = false,
}: {
  label: string
  value: number
  badWhenHigh?: boolean
}) {
  const warning = badWhenHigh ? value >= 60 : value <= 35
  return (
    <div className={`need-row ${warning ? "warning" : ""}`}>
      <div>
        <span>{label}</span>
        <strong>{value}%</strong>
      </div>
      <div className="need-track">
        <span style={{ width: `${value}%` }} />
      </div>
    </div>
  )
}
