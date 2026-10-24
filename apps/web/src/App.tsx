import { Fragment, lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react"

import { diagnostics, faultInjectionEnabled, installDiagnosticsHook } from "./diagnostics"
import {
  EFFECT_PIECES,
  loadEffectsManifest,
  type EffectPiece,
  type EffectsManifest,
} from "./effects-kit"
import type {
  ActionResult,
  CareCategory,
  CareTask,
  GateEvaluation,
  Guest,
  Keeper,
  PlacementEvaluation,
  Point,
  Snapshot,
  SpeciesKey,
  SpeciesOffer,
  Speed,
  Tool,
  Tutorial,
  WorkZoneEvaluation,
} from "./game-types"
import type { SceneOverlay } from "./park-scene"
import type { ParkPick } from "./ParkView"
import {
  loadSceneryManifest,
  SCENERY_PIECES,
  type SceneryManifest,
  type SceneryPiece,
} from "./scenery-manifest"
import init, { ZooGame } from "./wasm/zoo_core"
import { NO_ZONE_COLOR, zoneColor } from "./zone-colors"

/** How the selected work zone is being edited with the Zones tool. */
type ZoneEdit = "resize" | "move" | null

function idList(ids: number[]) {
  return ids.length === 0 ? "None" : ids.map((id) => `#${id}`).join(", ")
}

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

const CARE_CATEGORIES: CareCategory[] = ["feeding", "cleaning", "inspection", "enrichment"]
/** Whole hours 00:00–24:00 for keeper shift boundaries, in minutes of day. */
const SHIFT_HOURS = Array.from({ length: 25 }, (_, hour) => hour * 60)

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
    case "gate":
      return "Tap a fence segment of the selected habitat, then confirm the new service gate."
    case "zone":
      return "Drag corner to corner to draw a work zone · tap a zone to select it · zones may not overlap."
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
  // The checklist starts collapsed to one line on phones so the map stays tappable.
  const [tutorialOpen, setTutorialOpen] = useState(
    () => !window.matchMedia("(max-width: 900px)").matches,
  )
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
  const [gateCandidate, setGateCandidate] = useState<GateEvaluation | null>(null)
  const [selectedZoneId, setSelectedZoneId] = useState<number | null>(null)
  const [zoneEdit, setZoneEdit] = useState<ZoneEdit>(null)
  const [assignKeeperId, setAssignKeeperId] = useState<number | null>(null)
  const [cameraResetToken, setCameraResetToken] = useState(0)
  const [parkViewMount, setParkViewMount] = useState(0)
  const [startupFailure, setStartupFailure] = useState<string | null>(null)

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
    let stage: "wasm" | "asset" = "wasm"
    installDiagnosticsHook()
    init()
      .then(async () => {
        if (cancelled) {
          return
        }
        stage = "asset"
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
        if (faultInjectionEnabled()) {
          window.__zooFaults = {
            simulation: (kind: string) => {
              const result = callGame((game) => game.inject_fault(kind)) ?? ""
              diagnostics.retain()
              return result
            },
            remountParkView: () => setParkViewMount((mount) => mount + 1),
          }
        }
        refresh()
      })
      .catch((error: unknown) => {
        console.error("Zoo simulation initialization failed", error)
        if (!cancelled) {
          diagnostics.record(stage, String(error), "frozen")
          setStartupFailure(
            stage === "asset"
              ? `This build's park content does not match its simulation contract (${String(error)}).`
              : `The park simulation could not be loaded in this browser (${String(error)}).`,
          )
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
    clearFenceGesture()
    if (tool !== "zone") {
      setZoneEdit(null)
    }
  }, [clearFenceGesture, tool])

  useEffect(() => {
    setZoneEdit(null)
    setAssignKeeperId(null)
  }, [selectedZoneId])

  const selectedHabitat = useMemo(
    () => snapshot?.habitats.find((habitat) => habitat.id === selectedHabitatId) ?? null,
    [selectedHabitatId, snapshot],
  )

  const selectedGuest = useMemo(
    () => snapshot?.guests.find((guest) => guest.id === selectedGuestId) ?? null,
    [selectedGuestId, snapshot],
  )

  const selectedZone = useMemo(
    () => snapshot?.work_zones.find((zone) => zone.id === selectedZoneId) ?? null,
    [selectedZoneId, snapshot],
  )

  // Drawing, resizing, or moving a work zone: Rust evaluates the rectangle and lists the
  // service gates it would include. While moving, the tile under the pointer becomes the
  // zone's north-west corner.
  const zonePreview = useMemo(() => {
    const game = gameRef.current
    if (!game || tool !== "zone") {
      return null
    }
    let corners: [Point, Point] | null = null
    if (zoneEdit === "move" && selectedZone) {
      const anchor = fenceEnd ?? hoveredTile
      if (anchor) {
        corners = [
          anchor,
          { x: anchor.x + selectedZone.width - 1, y: anchor.y + selectedZone.height - 1 },
        ]
      }
    } else if (fenceStart && fenceEnd) {
      corners = [fenceStart, fenceEnd]
    }
    if (!corners) {
      return null
    }
    const [a, b] = corners
    const zoneId = zoneEdit && selectedZone ? selectedZone.id : 0
    const text = callGame(() => game.evaluate_work_zone(zoneId, a.x, a.y, b.x, b.y))
    return text === undefined ? null : (JSON.parse(text) as WorkZoneEvaluation)
  }, [callGame, fenceEnd, fenceStart, hoveredTile, selectedZone, snapshot, tool, zoneEdit])

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

  // Moving a gate: hovering previews the segment Rust resolves for that tile (desktop); a tap
  // pins the candidate until it is confirmed or cancelled (desktop and touch).
  const gateHover = useMemo(() => {
    const game = gameRef.current
    if (!game || tool !== "gate" || selectedHabitatId === null || !hoveredTile) {
      return null
    }
    const text = callGame(() =>
      game.evaluate_service_gate(selectedHabitatId, hoveredTile.x, hoveredTile.y),
    )
    return text === undefined ? null : (JSON.parse(text) as GateEvaluation)
  }, [callGame, hoveredTile, selectedHabitatId, snapshot, tool])

  useEffect(() => {
    if (tool !== "gate") {
      setGateCandidate(null)
    }
  }, [tool])

  useEffect(() => {
    setTool((current) => (current === "gate" ? "select" : current))
  }, [selectedHabitatId])

  const gatePreview = gateCandidate ?? (gateHover?.segment ? gateHover : null)

  const ghostTiles = useMemo(
    () => (snapshot && tool === "habitat" ? previewTiles(fenceStart, fenceEnd, snapshot) : []),
    [fenceEnd, fenceStart, snapshot?.width, snapshot?.height, tool],
  )

  const zonesVisible = tool === "zone" || selectedZoneId !== null

  const overlay = useMemo<SceneOverlay>(
    () => ({
      tool,
      hoveredTile,
      ghostTiles,
      placement,
      selectedHabitatId,
      selectedGuestId,
      selectedDepot,
      gatePreview,
      zonesVisible,
      selectedZoneId,
      zonePreview,
    }),
    [
      gatePreview,
      ghostTiles,
      hoveredTile,
      placement,
      selectedDepot,
      selectedGuestId,
      selectedHabitatId,
      selectedZoneId,
      tool,
      zonePreview,
      zonesVisible,
    ],
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
    } else if (tool === "habitat" || tool === "zone") {
      fenceStartRef.current = tile
      fenceEndRef.current = tile
      setFenceStart(tile)
      setFenceEnd(tile)
    }
  }

  const onTileDrag = (tile: Point) => {
    if (tool === "path") {
      paintPath(tile)
    } else if ((tool === "habitat" || tool === "zone") && fenceStartRef.current) {
      fenceEndRef.current = tile
      setFenceEnd(tile)
    }
  }

  const finishZoneGesture = (commit: boolean) => {
    const game = gameRef.current
    const start = fenceStartRef.current
    const end = fenceEndRef.current
    clearFenceGesture()
    if (!commit || !game || !snapshot || !start || !end) {
      return
    }
    const tap = start.x === end.x && start.y === end.y
    if (zoneEdit === "move" && selectedZone) {
      const result = perform(() =>
        game.set_work_zone_bounds(
          selectedZone.id,
          end.x,
          end.y,
          end.x + selectedZone.width - 1,
          end.y + selectedZone.height - 1,
        ),
      )
      if (result.ok) {
        setZoneEdit(null)
      }
      return
    }
    if (zoneEdit === "resize" && selectedZone) {
      if (tap) {
        inform("Drag from one corner to the opposite corner to resize the zone.")
        return
      }
      const result = perform(() =>
        game.set_work_zone_bounds(selectedZone.id, start.x, start.y, end.x, end.y),
      )
      if (result.ok) {
        setZoneEdit(null)
      }
      return
    }
    if (tap) {
      // Picking which drawn rectangle was tapped is presentation, not a rule decision.
      const zone = snapshot.work_zones.find(
        (candidate) =>
          end.x >= candidate.x &&
          end.x < candidate.x + candidate.width &&
          end.y >= candidate.y &&
          end.y < candidate.y + candidate.height,
      )
      selectZone(zone?.id ?? null)
      inform(
        zone
          ? `Work zone #${zone.id} selected`
          : "Drag from one corner to the opposite corner to draw a work zone.",
      )
      return
    }
    const result = perform(() => game.create_work_zone(start.x, start.y, end.x, end.y))
    if (result.ok) {
      const text = callGame((current) => current.snapshot_json())
      const zones = text === undefined ? [] : (JSON.parse(text) as Snapshot).work_zones
      selectZone(
        zones.reduce<number | null>((newest, zone) => Math.max(newest ?? 0, zone.id), null),
      )
    }
  }

  const onToolGestureEnd = (commit: boolean) => {
    paintedTilesRef.current.clear()
    if (tool === "zone") {
      finishZoneGesture(commit)
      return
    }
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
    setSelectedZoneId(null)
    setSelectedHabitatId(habitatId)
  }

  const selectZone = (zoneId: number | null) => {
    setSelectedGuestId(null)
    setSelectedDepot(false)
    setSelectedHabitatId(null)
    setSelectedZoneId(zoneId)
  }

  const selectDepot = () => {
    setSelectedGuestId(null)
    setSelectedHabitatId(null)
    setSelectedZoneId(null)
    setSelectedDepot(true)
    inform("Central operations depot selected · stock animal feed and hire park staff here.")
  }

  const skipTutorial = () => {
    const game = gameRef.current
    if (game) {
      perform(() => game.skip_tutorial())
    }
  }

  const restartTutorial = () => {
    const game = gameRef.current
    if (game) {
      perform(() => game.restart_tutorial())
      setTutorialOpen(true)
    }
  }

  /** The one object or tool the next training step needs, if the park has it. */
  const tutorialFocus = (): { label: string; run: () => void } | null => {
    const tutorial = snapshot?.tutorial
    if (!snapshot || !tutorial || tutorial.status !== "active") {
      return null
    }
    const habitat = snapshot.habitats.find((entry) => entry.id === tutorial.habitat_id)
    switch (tutorial.next) {
      case "build_habitat":
        return { label: "Draw habitat", run: () => setTool("habitat") }
      case "zone_gate":
        return { label: "Choose Zones", run: () => setTool("zone") }
      case "assign_keeper":
        if (snapshot.animal_care_depot.keepers.length === 0 || habitat?.work_zone_id == null) {
          return { label: "Open depot", run: selectDepot }
        }
        return {
          label: `Open zone #${habitat.work_zone_id}`,
          run: () => {
            setTool("select")
            selectZone(habitat.work_zone_id)
          },
        }
      case null:
        return null
      case "stock_habitat":
      case "job_generated":
      case "edit_schedule":
      case "job_completed":
        return habitat
          ? {
              label: `Show habitat #${habitat.id}`,
              run: () => {
                setTool("select")
                selectHabitat(habitat.id)
              },
            }
          : null
    }
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
          setSelectedZoneId(null)
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
    if (tool === "gate") {
      if (!target || selectedHabitatId === null) {
        return
      }
      const text = callGame(() => game.evaluate_service_gate(selectedHabitatId, target.x, target.y))
      if (text === undefined) {
        return
      }
      const evaluation = JSON.parse(text) as GateEvaluation
      setGateCandidate(evaluation.segment ? evaluation : null)
      setMessage(evaluation.message)
      setMessageKind(evaluation.ok ? "info" : "error")
      return
    }
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

  const setCarePriority = (task: CareTask, priority: number) => {
    const game = gameRef.current
    if (!game || selectedHabitatId === null) {
      return
    }
    perform(() =>
      game.set_care_task(
        selectedHabitatId,
        task.category,
        task.first_minute_of_day,
        task.interval_minutes,
        priority,
      ),
    )
  }

  const startGateMove = () => {
    setGateCandidate(null)
    setTool("gate")
    inform("Tap a fence segment of the selected habitat to preview its new service gate.")
  }

  const confirmGateMove = () => {
    const game = gameRef.current
    const segment = gateCandidate?.segment
    if (!game || selectedHabitatId === null || !segment) {
      return
    }
    const result = perform(() =>
      game.set_service_gate(selectedHabitatId, segment.x, segment.y, segment.side),
    )
    if (result.ok) {
      setTool("select")
    }
  }

  const startZoneEdit = (edit: Exclude<ZoneEdit, null>) => {
    setTool("zone")
    setZoneEdit(edit)
    clearFenceGesture()
    inform(
      edit === "resize"
        ? "Drag from one corner to the opposite corner to set the zone's new size."
        : "Tap or drag: the zone's north-west corner follows the tile under your pointer.",
    )
  }

  const deleteZone = () => {
    const game = gameRef.current
    if (!game || !selectedZone) {
      return
    }
    const result = perform(() => game.delete_work_zone(selectedZone.id))
    if (result.ok) {
      setSelectedZoneId(null)
    }
  }

  const assignKeeper = (keeperId: number) => {
    const game = gameRef.current
    if (!game || !selectedZone) {
      return
    }
    perform(() => game.assign_keeper_zone(keeperId, selectedZone.id))
  }

  const setKeeperSchedule = (
    keeper: Keeper,
    change: { categories?: CareCategory[]; start?: number; end?: number },
  ) => {
    const game = gameRef.current
    if (!game) {
      return
    }
    const categories = change.categories ?? keeper.categories
    perform(() =>
      game.set_keeper_schedule(
        keeper.id,
        categories.join(","),
        change.start ?? keeper.shift_start_minute,
        change.end ?? keeper.shift_end_minute,
      ),
    )
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
    setSelectedZoneId(null)
    setHoveredTile(null)
    setCameraResetToken((token) => token + 1)
    setMessage("New park started.")
    setMessageKind("info")
    refresh()
  }

  if (!snapshot) {
    return (
      <main className="loading">
        {startupFailure === null ? (
          "Preparing the park simulation…"
        ) : (
          <section className="recovery-panel startup-failure bevel" aria-label="Park recovery">
            <div className="window-title">
              <span>Park could not start</span>
            </div>
            <p role="alert">{startupFailure}</p>
            <p>Export the diagnostics, then reload once an updated build is available.</p>
            <div className="recovery-actions">
              <button onClick={downloadDiagnostics}>Download diagnostics</button>
              <button onClick={copyDiagnostics}>Copy diagnostics</button>
            </div>
            {manualExport !== null && (
              <textarea aria-label="Diagnostics bundle" readOnly value={manualExport} rows={4} />
            )}
          </section>
        )}
      </main>
    )
  }

  const zoneTooltip =
    zonePreview && hoveredTile ? (
      <div className={zonePreview.ok ? "valid" : "invalid"}>
        <b>
          {zonePreview.ok ? "✓ Work zone" : "! Cannot place zone"} · {zonePreview.bounds.width}×
          {zonePreview.bounds.height}
        </b>
        <small>{zonePreview.message}</small>
      </div>
    ) : undefined

  const tooltip =
    zoneTooltip ??
    (placement && hoveredTile ? (
      <div className={placement.ok ? "valid" : "invalid"}>
        <b>
          {placement.ok ? "✓ Fence closes" : "! Cannot build"} · {placement.width}×
          {placement.height} · {money(placement.cost_cents)}
        </b>
        <small>{placement.message}</small>
      </div>
    ) : undefined)

  const focus = tutorialFocus()
  const uncoveredHabitats = snapshot.habitats.filter((habitat) => habitat.work_zone_id === null)
  const keeperChoice =
    selectedZone === null
      ? null
      : (snapshot.animal_care_depot.keepers.find((keeper) => keeper.id === assignKeeperId) ??
        snapshot.animal_care_depot.keepers.find((keeper) => keeper.needs_zone) ??
        snapshot.animal_care_depot.keepers.find((keeper) => keeper.zone_id !== selectedZone.id) ??
        null)

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
            key={parkViewMount}
            snapshot={snapshot}
            overlay={overlay}
            paused={speed === 0}
            dragTool={tool === "path" || tool === "habitat" || tool === "zone"}
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

        <aside
          className={`side-panel bevel${tool === "gate" ? " gate-mode" : ""}${
            tool === "zone" && zoneEdit !== null ? " zone-edit-mode" : ""
          }`}
        >
          {snapshot.tutorial.status === "active" && (
            <TutorialChecklist
              tutorial={snapshot.tutorial}
              open={tutorialOpen}
              onToggle={setTutorialOpen}
              focusLabel={focus?.label ?? null}
              onFocus={() => focus?.run()}
              onSkip={skipTutorial}
            />
          )}
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
                    <small>Assign new keepers to a work zone; they take its due care work</small>
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
                      <div className={keeper.needs_zone ? "zone-warning" : ""}>
                        <dt>Work zone</dt>
                        <dd>
                          {keeper.zone_id === null ? "Needs a zone" : `Zone #${keeper.zone_id}`}
                        </dd>
                      </div>
                      <div>
                        <dt>Accepts</dt>
                        <dd className="keeper-categories">
                          {CARE_CATEGORIES.map((category) => {
                            const accepted = keeper.categories.includes(category)
                            return (
                              <label key={category}>
                                <input
                                  type="checkbox"
                                  checked={accepted}
                                  disabled={accepted && keeper.categories.length === 1}
                                  onChange={() =>
                                    setKeeperSchedule(keeper, {
                                      categories: accepted
                                        ? keeper.categories.filter((item) => item !== category)
                                        : [...keeper.categories, category],
                                    })
                                  }
                                />{" "}
                                {category}
                              </label>
                            )
                          })}
                        </dd>
                      </div>
                      <div className={keeper.on_shift ? "" : "zone-warning"}>
                        <dt>Shift</dt>
                        <dd>
                          <select
                            aria-label={`Keeper #${keeper.id} shift start`}
                            value={keeper.shift_start_minute}
                            onChange={(event) =>
                              setKeeperSchedule(keeper, { start: Number(event.target.value) })
                            }
                          >
                            {SHIFT_HOURS.slice(0, 24).map((minute) => (
                              <option key={minute} value={minute}>
                                {clock(minute)}
                              </option>
                            ))}
                          </select>
                          {" – "}
                          <select
                            aria-label={`Keeper #${keeper.id} shift end`}
                            value={keeper.shift_end_minute}
                            onChange={(event) =>
                              setKeeperSchedule(keeper, { end: Number(event.target.value) })
                            }
                          >
                            {SHIFT_HOURS.slice(1).map((minute) => (
                              <option key={minute} value={minute}>
                                {clock(minute)}
                              </option>
                            ))}
                          </select>
                        </dd>
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
          {!selectedDepot && !selectedGuest && !selectedHabitat && selectedZone && (
            <>
              <div className="window-title">
                <span>
                  <i
                    className="zone-swatch"
                    style={{ background: zoneColor(selectedZone.id) }}
                    aria-hidden="true"
                  />
                  Work zone #{selectedZone.id}
                </span>
                <button onClick={() => setSelectedZoneId(null)}>×</button>
              </div>
              <div className="manager-card zone-card">
                <dl>
                  <div>
                    <dt>Area</dt>
                    <dd>
                      {selectedZone.width}×{selectedZone.height} from ({selectedZone.x},{" "}
                      {selectedZone.y})
                    </dd>
                  </div>
                  <div>
                    <dt>Service gates</dt>
                    <dd>{idList(selectedZone.habitat_ids)}</dd>
                  </div>
                  <div>
                    <dt>Keepers</dt>
                    <dd>{idList(selectedZone.keeper_ids)}</dd>
                  </div>
                </dl>
                {uncoveredHabitats.length > 0 && (
                  <div className="guest-thought zone-warning">
                    No zone: habitats {idList(uncoveredHabitats.map((habitat) => habitat.id))} have
                    a service gate outside every work zone.
                  </div>
                )}
                {zoneEdit !== null ? (
                  <div className="zone-edit-controls">
                    <div className="guest-thought">
                      {zonePreview?.message ??
                        (zoneEdit === "resize"
                          ? "Drag from one corner to the opposite corner to set the new size."
                          : "Tap or drag: the zone's north-west corner follows your pointer.")}
                    </div>
                    <button className="shop-row" onClick={() => setZoneEdit(null)}>
                      <span>
                        <b>Cancel</b>
                        <small>Keep the current zone bounds</small>
                      </span>
                    </button>
                  </div>
                ) : (
                  <>
                    <button className="shop-row" onClick={() => startZoneEdit("resize")}>
                      <span>
                        <b>Resize</b>
                        <small>Drag a new rectangle for this zone</small>
                      </span>
                    </button>
                    <button className="shop-row" onClick={() => startZoneEdit("move")}>
                      <span>
                        <b>Move</b>
                        <small>Keep the size and pick a new north-west corner</small>
                      </span>
                    </button>
                    <button className="shop-row" onClick={deleteZone}>
                      <span>
                        <b>Delete</b>
                        <small>Its keepers become unassigned and idle</small>
                      </span>
                    </button>
                  </>
                )}
                <h3>Assign keeper</h3>
                {snapshot.animal_care_depot.keepers.length === 0 ? (
                  <div className="guest-thought">Hire keepers at the central operations depot.</div>
                ) : (
                  <>
                    <label className="zone-keeper-select">
                      <span>Keeper</span>
                      <select
                        value={keeperChoice?.id ?? ""}
                        onChange={(event) => setAssignKeeperId(Number(event.target.value))}
                      >
                        {snapshot.animal_care_depot.keepers.map((keeper) => (
                          <option key={keeper.id} value={keeper.id}>
                            Keeper #{keeper.id} ·{" "}
                            {keeper.zone_id === null ? "needs a zone" : `zone #${keeper.zone_id}`}
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
                      className="shop-row"
                      disabled={!keeperChoice}
                      onClick={() => keeperChoice && assignKeeper(keeperChoice.id)}
                    >
                      <span>
                        <b>Assign keeper</b>
                        <small>Keepers only take care work in their own zone</small>
                      </span>
                    </button>
                  </>
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

                <div className="gate-controls">
                  <h3>Service gate</h3>
                  <dl>
                    <div>
                      <dt>Gate</dt>
                      <dd>
                        {`${selectedHabitat.service_gate.side} fence at (${selectedHabitat.service_gate.x}, ${selectedHabitat.service_gate.y})`}
                      </dd>
                    </div>
                    <div>
                      <dt>Keeper access</dt>
                      <dd>
                        {selectedHabitat.service_gate.reachable ? "Reachable" : "Gate unreachable"}
                      </dd>
                    </div>
                    <div className={selectedHabitat.work_zone_id === null ? "zone-warning" : ""}>
                      <dt>Work zone</dt>
                      <dd>
                        {selectedHabitat.work_zone_id === null ? (
                          <>
                            <i
                              className="zone-swatch"
                              style={{ background: NO_ZONE_COLOR }}
                              aria-hidden="true"
                            />
                            No zone
                          </>
                        ) : (
                          <>
                            <i
                              className="zone-swatch"
                              style={{ background: zoneColor(selectedHabitat.work_zone_id) }}
                              aria-hidden="true"
                            />
                            Zone #{selectedHabitat.work_zone_id}
                          </>
                        )}
                      </dd>
                    </div>
                  </dl>
                  {tool === "gate" ? (
                    <>
                      <div className="guest-thought">
                        {gateCandidate
                          ? gateCandidate.message
                          : "Tap a fence segment of this habitat to preview the new gate."}
                      </div>
                      <button
                        className="shop-row"
                        disabled={!gateCandidate?.ok}
                        onClick={confirmGateMove}
                      >
                        <span>
                          <b>Place gate here</b>
                          <small>Keepers enter through the previewed segment</small>
                        </span>
                      </button>
                      <button className="shop-row" onClick={() => setTool("select")}>
                        <span>
                          <b>Cancel</b>
                          <small>Keep the current gate</small>
                        </span>
                      </button>
                    </>
                  ) : (
                    <button className="shop-row" onClick={startGateMove}>
                      <span>
                        <b>Move gate</b>
                        <small>Then tap a fence segment; the tile outside stays clear</small>
                      </span>
                    </button>
                  )}
                </div>

                <h3>Care</h3>
                <div className="guest-thought">{selectedHabitat.care_status}</div>
                <dl>
                  <div className={selectedHabitat.zone_keeper_count === 0 ? "zone-warning" : ""}>
                    <dt>Zone keepers</dt>
                    <dd>
                      {selectedHabitat.work_zone_id === null
                        ? "No zone"
                        : `${selectedHabitat.zone_keeper_count} in zone #${selectedHabitat.work_zone_id}`}
                    </dd>
                  </div>
                  {selectedHabitat.care_required_keepers !== null &&
                    selectedHabitat.care_required_keepers > 1 && (
                      <div
                        className={
                          selectedHabitat.zone_keeper_count < selectedHabitat.care_required_keepers
                            ? "zone-warning"
                            : ""
                        }
                      >
                        <dt>Dangerous animals</dt>
                        <dd>
                          {selectedHabitat.care_required_keepers} keepers together per job
                          {selectedHabitat.zone_keeper_count <
                            selectedHabitat.care_required_keepers &&
                            ` · only ${selectedHabitat.zone_keeper_count} in zone`}
                        </dd>
                      </div>
                    )}
                  <div>
                    <dt>Missed care (24 h)</dt>
                    <dd>
                      {selectedHabitat.care_jobs.filter((job) => job.state === "missed").length}
                    </dd>
                  </div>
                </dl>
                <div className="guest-thought">{selectedHabitat.feeding_status}</div>
                {selectedHabitat.care_schedule.length === 0 ? (
                  <div className="guest-thought">
                    The care schedule starts from species defaults with the first animals.
                  </div>
                ) : (
                  <dl>
                    {selectedHabitat.care_schedule.map((task) => {
                      const due = selectedHabitat.care_jobs.find(
                        (job) => job.category === task.category && job.state === "due",
                      )
                      const active = selectedHabitat.care_jobs.find(
                        (job) => job.category === task.category && job.state === "in_progress",
                      )
                      let progress = `next in ${task.next_due_in_minutes} min`
                      if (active) {
                        const crew = active.keeper_ids.length > 1 ? "keepers" : "keeper"
                        progress = `${crew} ${idList(active.keeper_ids)} on the job`
                      } else if (due) {
                        progress = `due, ${due.expires_in_minutes} min left`
                        if (due.waiting_reason !== null) {
                          progress += ` (${due.waiting_reason})`
                        }
                      }
                      return (
                        <div key={task.category}>
                          <dt>
                            {task.category}
                            {task.customized ? " *" : ""}
                          </dt>
                          <dd>
                            from {clock(task.first_minute_of_day)} every{" "}
                            {task.interval_minutes / 60} h · {task.work_minutes} min work ·{" "}
                            {progress} · priority{" "}
                            <button
                              disabled={task.priority <= 1}
                              aria-label={`Lower ${task.category} priority`}
                              onClick={() => setCarePriority(task, task.priority - 1)}
                            >
                              −
                            </button>{" "}
                            {task.priority}{" "}
                            <button
                              disabled={task.priority >= 5}
                              aria-label={`Raise ${task.category} priority`}
                              onClick={() => setCarePriority(task, task.priority + 1)}
                            >
                              +
                            </button>
                          </dd>
                        </div>
                      )
                    })}
                  </dl>
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
                {selectedHabitat.zone_keeper_count === 0 && (
                  <div className="guest-thought">
                    Assign a keeper to this habitat's work zone before animals can move in.
                  </div>
                )}
                {snapshot.species_catalog.map((offer) => {
                  const wrongSpecies =
                    selectedHabitat.species !== null && selectedHabitat.species !== offer.key
                  const full = selectedHabitat.animals >= selectedHabitat.capacity
                  const unstaffed = selectedHabitat.zone_keeper_count === 0
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
          {!selectedDepot && !selectedGuest && !selectedHabitat && !selectedZone && (
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
                  <li>
                    Follow the keeper training checklist to zone, staff and stock the first habitat.
                  </li>
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
                {snapshot.tutorial.status !== "active" && (
                  <button className="secondary" onClick={restartTutorial}>
                    Restart keeper training
                  </button>
                )}
                <button className="secondary" onClick={reset}>
                  Start new park
                </button>
                <SceneryCredits />
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
          <ToolButton
            active={tool === "zone"}
            icon="▢"
            label="Zones"
            onClick={() => setTool("zone")}
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

const SCENERY_LABELS: Record<SceneryPiece, string> = {
  "tree-round": "Round tree",
  "tree-pine": "Pine tree",
  "tree-acacia": "Acacia",
  "bush-round": "Berry bush",
  "grass-tuft": "Grass tuft",
  "rock-round": "Rock",
  "rock-boulder": "Boulder",
}

const EFFECT_LABELS: Record<EffectPiece, string> = {
  dust: "Keeper dust",
  leaf: "Leaf flutter",
  splash: "Pond splash",
}

/** Lineage of the asset-tooling scenery around the park, as pinned and verified at build time. */
function SceneryCredits() {
  const [manifest, setManifest] = useState<SceneryManifest | null>(null)
  const [effects, setEffects] = useState<EffectsManifest | null>(null)
  useEffect(() => {
    let cancelled = false
    // A failed load is reported by the park view, which cannot render without the scenery.
    loadSceneryManifest().then(
      (loaded) => !cancelled && setManifest(loaded),
      () => undefined,
    )
    loadEffectsManifest().then(
      (loaded) => !cancelled && setEffects(loaded),
      () => undefined,
    )
    return () => {
      cancelled = true
    }
  }, [])
  if (!manifest) {
    return null
  }
  const commit = manifest.assetToolingCommit
  return (
    <details className="scenery-credits" aria-label="Scenery credits">
      <summary>Scenery credits</summary>
      <p>
        Trees, bushes, grass and rocks around and inside the park come from the asset-tooling{" "}
        <a
          href={`https://github.com/moritzbrantner/asset-tooling/releases/tag/${manifest.release}`}
          target="_blank"
          rel="noreferrer"
        >
          {manifest.release}
        </a>{" "}
        release, built by{" "}
        <a
          href={`https://github.com/moritzbrantner/asset-tooling/blob/${commit}/${manifest.recipe.script}`}
          target="_blank"
          rel="noreferrer"
        >
          {manifest.recipe.script}
        </a>{" "}
        in Blender {manifest.recipe.blenderVersion} at asset-tooling{" "}
        <code>{commit.slice(0, 7)}</code>.
      </p>
      <p>Provenance: {manifest.provenance}.</p>
      <div className="finance-grid">
        <span>Release tar</span>
        <code title={manifest.tarSha256}>{manifest.tarSha256.slice(0, 12)}</code>
        <span>Recipe</span>
        <code title={manifest.recipe.sha256}>{manifest.recipe.sha256.slice(0, 12)}</code>
        {SCENERY_PIECES.map((piece) => (
          <Fragment key={piece}>
            <span>
              {SCENERY_LABELS[piece]} · {manifest.pieces[piece].key}/
              {manifest.pieces[piece].variant}
            </span>
            <code title={manifest.pieces[piece].sha256}>
              {manifest.pieces[piece].sha256.slice(0, 12)}
            </code>
          </Fragment>
        ))}
      </div>
      {effects && (
        <>
          <p>
            Dust, leaf and splash particles come from the asset-tooling{" "}
            <a
              href={`https://github.com/moritzbrantner/asset-tooling/releases/tag/${effects.release}`}
              target="_blank"
              rel="noreferrer"
            >
              {effects.release}
            </a>{" "}
            release, baked by{" "}
            <a
              href={`https://github.com/moritzbrantner/asset-tooling/blob/${effects.assetToolingCommit}/${effects.recipe.source}`}
              target="_blank"
              rel="noreferrer"
            >
              {effects.recipe.source}
            </a>{" "}
            at asset-tooling <code>{effects.assetToolingCommit.slice(0, 7)}</code>. They are
            cosmetic and off under reduced motion.
          </p>
          <p>Provenance: {effects.provenance}.</p>
          <div className="finance-grid">
            <span>Release tar</span>
            <code title={effects.tarSha256}>{effects.tarSha256.slice(0, 12)}</code>
            <span>Recipe</span>
            <code title={effects.recipe.sha256}>{effects.recipe.sha256.slice(0, 12)}</code>
            {EFFECT_PIECES.map((piece) => (
              <Fragment key={piece}>
                <span>
                  {EFFECT_LABELS[piece]} · {effects.pieces[piece].key}/
                  {effects.pieces[piece].variant}
                </span>
                <code title={effects.pieces[piece].sha256}>
                  {effects.pieces[piece].sha256.slice(0, 12)}
                </code>
              </Fragment>
            ))}
          </div>
        </>
      )}
    </details>
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

/** The compact keeper-training checklist; every tick comes from Rust park state. */
function TutorialChecklist({
  tutorial,
  open,
  onToggle,
  focusLabel,
  onFocus,
  onSkip,
}: {
  tutorial: Tutorial
  open: boolean
  onToggle: (open: boolean) => void
  focusLabel: string | null
  onFocus: () => void
  onSkip: () => void
}) {
  const doneCount = tutorial.steps.filter((step) => step.done).length
  return (
    <details
      className="tutorial-card bevel"
      open={open}
      onToggle={(event) => onToggle(event.currentTarget.open)}
      aria-label="Keeper training checklist"
    >
      <summary>
        Keeper training · {doneCount}/{tutorial.steps.length}
      </summary>
      <ol>
        {tutorial.steps.map((step) => (
          <li
            key={step.key}
            data-step={step.key}
            className={[step.done && "done", step.key === tutorial.next && "next"]
              .filter(Boolean)
              .join(" ")}
            aria-current={step.key === tutorial.next ? "step" : undefined}
          >
            <span aria-hidden="true">{step.done ? "✓" : "○"}</span> {step.title}
          </li>
        ))}
      </ol>
      {tutorial.lesson !== null && <p>{tutorial.lesson}</p>}
      {tutorial.problem !== null && (
        <p className="tutorial-problem" role="status">
          {tutorial.problem}
        </p>
      )}
      <div className="tutorial-actions">
        {focusLabel !== null && <button onClick={onFocus}>{focusLabel}</button>}
        <button onClick={onSkip}>Skip training</button>
      </div>
    </details>
  )
}
