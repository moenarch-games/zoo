import {
  Fragment,
  lazy,
  type ReactNode,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react"

import { AnimalHealthPanel } from "./AnimalHealth"
import {
  ConditionsPanel,
  conditionsLabel,
  loadDisplaySettings,
  storeDisplaySettings,
  type DisplaySettings,
} from "./ConditionsPanel"
import { diagnostics, faultInjectionEnabled, installDiagnosticsHook } from "./diagnostics"
import { EducationPanel, type EducationCommands } from "./EducationPanel"
import {
  EFFECT_PIECES,
  loadEffectsManifest,
  type EffectPiece,
  type EffectsManifest,
} from "./effects-kit"
import { FinancePanel, type FinanceCommands } from "./FinancePanel"
import type {
  ActionResult,
  ComfortFacility,
  ComfortOperations,
  AdmissionEvaluation,
  BudgetEvaluation,
  Alert,
  Animal,
  AlertAction,
  ArrivalPreview,
  CareCategory,
  CareTask,
  ConstructionPreview,
  ConstructionStep,
  EnvironmentPreview,
  FacilityOffer,
  GateEvaluation,
  Guest,
  Habitat,
  Keeper,
  LandQuote,
  Objective,
  OverlayKind,
  OverlayView,
  PathAccess,
  PlacementEvaluation,
  Point,
  QuarantinePreview,
  ScenarioState,
  ScenarioSummary,
  Snapshot,
  SpeciesKey,
  SpeciesOffer,
  Speed,
  Tool,
  Tutorial,
  WorkZoneEvaluation,
} from "./game-types"
import { GuestVisit } from "./GuestVisit"
import { HabitatDesignPanel, type HabitatBrush } from "./HabitatDesignPanel"
import {
  LandPanel,
  parcelAt,
  parcelCenter,
  type PlanEvaluation,
  type PlanItem,
  PlanPanel,
} from "./LandPanel"
import {
  AlertList,
  DailySummaryView,
  type JumpTarget,
  ManagementWorkspace,
  OVERLAY_KINDS,
  OverlayLegend,
  type WorkspaceTab,
} from "./ManagementWorkspace"
import { AdoptionOffers, AnimalMoves, ResidentGroups } from "./MixedSpecies"
import type { SceneOverlay } from "./park-scene"
import {
  background,
  loadFailureMessage,
  type SaveSummary,
  type SlotEntry,
  summarizeDocument,
} from "./park-storage"
import { ParkLibraryPanel, type PanelPrompt } from "./ParkLibraryPanel"
import type { CameraRequest, ParkPick } from "./ParkView"
import { PricingPanel, type PricingCommands } from "./PricingPanel"
import {
  loadSceneryManifest,
  SCENERY_PIECES,
  type SceneryManifest,
  type SceneryPiece,
} from "./scenery-manifest"
import { StandActions, StandRows } from "./StandPanel"
import { type LoadCheck, useParkLibrary } from "./use-park-library"
import { HabitatUtilitiesPanel, UtilityPlantActions, UtilityPlantRows } from "./UtilityPanel"
import init, { scenario_catalog_json, ZooGame } from "./wasm/zoo_core"
import { NO_ZONE_COLOR, zoneColor } from "./zone-colors"

/** How the selected work zone is being edited with the Zones tool. */
type ZoneEdit = "resize" | "move" | null

const DEFAULT_SCENARIO_ID = "empty-starter"

/** The opening line for a new park: keeper training, or its first objective. */
function scenarioHint(scenario: ScenarioState) {
  const first = scenario.objectives.find((objective) => !objective.done)
  if (first) {
    return `First objective: ${first.title}.`
  }
  return "Extend the entrance path, draw a habitat, then stock and staff it through the care depot."
}

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

/** A habitat's resident species, in arrival order (#108), or "Empty habitat". */
function habitatSpeciesLabel(habitat: Habitat, catalog: SpeciesOffer[]) {
  if (habitat.species.length === 0) {
    return "Empty habitat"
  }
  return habitat.species.map((key) => speciesLabel(key, catalog)).join(" & ")
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
    case "walking_to_facility":
      return "Heading to a facility"
    case "waiting_for_facility":
      return "Waiting for a free spot"
    case "using_facility":
      return "Using a facility"
  }
}

/** Build-tool glyphs for the facility services the park can build (#101, #113). */
const FACILITY_ICONS: Partial<Record<FacilityOffer["service"], string>> = {
  food: "▰",
  drink: "▥",
  toilet: "WC",
  bench: "⑃",
  bin: "♻",
  water_pump: "≋",
  climate_plant: "❄",
  sign: "ℹ",
}

/** The facility an alert's build fix places. */
const BUILD_ACTION_SERVICE = {
  build_toilet: "toilet",
  build_bench: "bench",
  build_water_pump: "water_pump",
  build_climate_plant: "climate_plant",
} as const satisfies Partial<Record<Alert["action"], FacilityOffer["service"]>>

function toolHint(tool: Tool, snapshot: Snapshot | null) {
  if (tool.startsWith("facility:")) {
    const offer = snapshot?.facility_catalog.find((entry) => entry.key === tool.slice(9))
    return offer
      ? `Click clear grass beside a path to build ${offer.label} · ${money(offer.build_cost_cents)}.`
      : "Select an available facility."
  }
  switch (tool) {
    case "path":
      return "Drag to paint a path stroke (or drag a plaza rectangle) · public paths are for guests, staff-only paths for keepers, janitors and mechanics · painting over the other kind reclassifies it · Esc cancels · Ctrl+Z undoes."
    case "habitat":
      return "Rectangle: drag corner to corner, or tap to place the size below · Outline: drag or tap along grid corners and close back at the first · Esc cancels · Ctrl+Z undoes."
    case "reshape":
      return "Draw the habitat's new fence along grid corners and close it at the first corner · animals, care and staff stay · Esc cancels · Ctrl+Z undoes."
    case "bulldoze":
      return "Hover to see what demolition removes and whether it can be undone · click to demolish."
    case "gate":
      return "Tap a fence segment of the selected habitat, then confirm the new service gate."
    case "zone":
      return "Drag corner to corner to draw a work zone · tap a zone to select it · zones may not overlap."
    case "terrain":
      return "Drag inside the selected habitat to paint ground or water or set a height; tap to place a furnishing · Rust previews cost and welfare · Esc cancels."
    case "land":
      return "Tap a parcel to compare its price, space and access · select several to buy them together · Clear cancels the selection."
    case "select":
      return "Click to inspect · drag to pan · right-drag or two-finger twist to rotate · scroll or pinch to zoom."
  }
}

function constructionHeadline(preview: ConstructionPreview) {
  const demolish = preview.action === "demolish"
  if (!preview.ok) {
    return demolish ? "! Cannot demolish" : "! Cannot build"
  }
  if (!preview.changes) {
    return "No change"
  }
  const price = preview.cost_cents > 0 ? ` · ${money(preview.cost_cents)}` : ""
  return `${demolish ? "✓ Demolish" : "✓ Build"}${price}`
}

function constructionDetails(preview: ConstructionPreview) {
  const effects = preview.affected.map((entry) => ` · ${entry.effect}`).join("")
  if (!preview.changes) {
    return `${preview.message}${effects}`
  }
  return `${preview.message}${effects} · ${preview.undoable ? "undoable" : "cannot be undone"}`
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
  // The path stroke of the active gesture, in pointer order; Rust joins and plans it.
  const strokeRef = useRef<Point[]>([])
  const [stroke, setStroke] = useState<Point[]>([])
  const [habitatSize, setHabitatSize] = useState({ width: 4, height: 3 })
  // The path brush (#98): a stroke 1–3 tiles wide or a dragged plaza rectangle, public or
  // staff-only. Rust plans the footprint, price and reclassification.
  const [pathBrush, setPathBrush] = useState<{
    shape: "stroke" | "plaza"
    width: number
    access: PathAccess
  }>({ shape: "stroke", width: 1, access: "public" })
  // A fence outline in progress: grid corners in drawing order, kept across taps and drags
  // until it closes on its first corner; Rust joins, closes and plans it (#95).
  const [habitatShape, setHabitatShape] = useState<"rect" | "outline">("rect")
  const outlineRef = useRef<Point[]>([])
  const [outline, setOutline] = useState<Point[]>([])
  const fenceStartRef = useRef<Point | null>(null)
  const fenceEndRef = useRef<Point | null>(null)
  // Land (#99): the parcels selected for one purchase, and the non-binding expansion plan of
  // prospective paths and habitats. Neither is saved; Rust quotes and evaluates both.
  const [landSelection, setLandSelection] = useState<string[]>([])
  const [planning, setPlanning] = useState(false)
  const [plan, setPlan] = useState<PlanItem[]>([])
  const planIdRef = useRef(0)

  const [snapshot, setSnapshot] = useState<Snapshot | null>(null)
  const [tool, setTool] = useState<Tool>("select")
  // What the terrain tool applies inside the selected habitat (#97).
  const [brush, setBrush] = useState<HabitatBrush>({ kind: "surface", surface: "sand" })
  const [speed, setSpeed] = useState<Speed>(1)
  // The checklist starts collapsed to one line on phones so the map stays tappable.
  const [tutorialOpen, setTutorialOpen] = useState(
    () => !window.matchMedia("(max-width: 900px)").matches,
  )
  const [message, setMessage] = useState("Continue a saved park, or start a new one.")
  const [messageKind, setMessageKind] = useState<"info" | "error">("info")
  const [selectedHabitatId, setSelectedHabitatId] = useState<number | null>(null)
  const [selectedGuestId, setSelectedGuestId] = useState<number | null>(null)
  const [selectedAnimalId, setSelectedAnimalId] = useState<number | null>(null)
  // The animal the camera watches and follows (#141); presentation only, never game state.
  const [watchedAnimalId, setWatchedAnimalId] = useState<number | null>(null)
  const [animalName, setAnimalName] = useState("")
  // Adoption group size and the selected animal's transfer destination (#108).
  const [adoptCount, setAdoptCount] = useState(1)
  const [transferTarget, setTransferTarget] = useState<number | null>(null)
  const [selectedDepot, setSelectedDepot] = useState(false)
  const [selectedConcessionId, setSelectedConcessionId] = useState<number | null>(null)
  // #94: the management workspace (null when closed) and the active overlay.
  const [workspaceTab, setWorkspaceTab] = useState<WorkspaceTab | null>(null)
  // Lighting and weather visuals (#118): browser display settings, never game rules.
  const [display, setDisplay] = useState<DisplaySettings>(loadDisplaySettings)
  const changeDisplay = (settings: DisplaySettings) => {
    setDisplay(settings)
    storeDisplaySettings(settings)
  }
  const [overlayKind, setOverlayKind] = useState<OverlayKind | null>(null)
  const [hoveredTile, setHoveredTile] = useState<Point | null>(null)
  const [fenceStart, setFenceStart] = useState<Point | null>(null)
  const [fenceEnd, setFenceEnd] = useState<Point | null>(null)
  const [gateCandidate, setGateCandidate] = useState<GateEvaluation | null>(null)
  const [selectedZoneId, setSelectedZoneId] = useState<number | null>(null)
  const [zoneEdit, setZoneEdit] = useState<ZoneEdit>(null)
  const [assignKeeperId, setAssignKeeperId] = useState<number | null>(null)
  const [camera, setCamera] = useState<CameraRequest>({ token: 0, focus: null })
  const requestCamera = (focus: Point | null) =>
    setCamera((current) => ({ token: current.token + 1, focus }))
  const [parkViewMount, setParkViewMount] = useState(0)
  const [startupFailure, setStartupFailure] = useState<string | null>(null)
  // #89: the start screen shows once the simulation is ready; play begins with a new, loaded
  // or imported park. The park menu is the in-game library dialog.
  const [gameReady, setGameReady] = useState(false)
  const scenarios = useMemo<ScenarioSummary[]>(
    () => (gameReady ? (JSON.parse(scenario_catalog_json()) as ScenarioSummary[]) : []),
    [gameReady],
  )
  const [screen, setScreen] = useState<"start" | "play">("start")
  const [menuOpen, setMenuOpen] = useState(false)
  const [menuView, setMenuView] = useState<"main" | "new">("main")
  const [prompt, setPrompt] = useState<PanelPrompt | null>(null)

  const notify = useCallback((text: string, kind: "info" | "error" = "info") => {
    setMessage(text)
    setMessageKind(kind)
  }, [])

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
    if (text === undefined) {
      return null
    }
    const next = JSON.parse(text) as Snapshot
    setSnapshot(next)
    return next
  }, [callGame])

  const captureSave = useCallback(
    () => (stoppedRef.current ? undefined : callGame((game) => game.save_json())),
    [callGame],
  )

  /** Imports are checked by zoo-core on a scratch park, so the active park never changes. */
  const validateSave = useCallback((document: string): LoadCheck => {
    let scratch: ZooGame | null = null
    try {
      scratch = new ZooGame()
      return JSON.parse(scratch.load_save(document)) as LoadCheck
    } catch (error) {
      return { ok: false, message: String(error) }
    } finally {
      scratch?.free()
    }
  }, [])

  const library = useParkLibrary({ captureSave, validateSave, notify })
  const { autosaveIfDue, markChanged } = library
  // A competing tab's newer save pauses autosave; surface the choice instead of overwriting.
  useEffect(() => {
    if (library.conflict) {
      setMenuOpen(true)
    }
  }, [library.conflict])

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
      markChanged()
      setMessage(result.message)
      setMessageKind(result.ok ? "info" : "error")
      refresh()
      checkFrozen()
      return result
    },
    [callGame, checkFrozen, markChanged, refresh],
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
        // Read-only: the complete authoritative park, so browser proofs can show that camera
        // and watch presentation never change simulation state (#141).
        window.__zooSimulation = {
          stateJson: () => gameRef.current?.save_json() ?? null,
          snapshotJson: () => gameRef.current?.snapshot_json() ?? null,
        }
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
        setGameReady(true)
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
  }, [callGame])

  useEffect(() => {
    if (!snapshot || speed === 0 || screen !== "play") {
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
      markChanged()
      const next = refresh()
      if (next) {
        autosaveIfDue((next.day - 1) * 24 * 60 + next.minute_of_day)
      }
    }, 250)
    return () => window.clearInterval(handle)
  }, [autosaveIfDue, callGame, checkFrozen, markChanged, refresh, screen, snapshot !== null, speed])

  const clearStroke = useCallback(() => {
    strokeRef.current = []
    setStroke([])
  }, [])

  const clearOutline = useCallback(() => {
    outlineRef.current = []
    setOutline([])
  }, [])

  const outlineMode = tool === "reshape" || (tool === "habitat" && habitatShape === "outline")

  useEffect(() => {
    clearStroke()
    clearFenceGesture()
    clearOutline()
    if (tool !== "zone") {
      setZoneEdit(null)
    }
  }, [clearFenceGesture, clearOutline, clearStroke, habitatShape, pathBrush.shape, tool])

  const plazaMode = tool === "path" && pathBrush.shape === "plaza"

  useEffect(() => {
    setZoneEdit(null)
    setAssignKeeperId(null)
  }, [selectedZoneId])

  const selectedHabitat = useMemo(
    () => snapshot?.habitats.find((habitat) => habitat.id === selectedHabitatId) ?? null,
    [selectedHabitatId, snapshot],
  )

  // Rust's plan for rebuilding the selected habitat's fence as each other fence type.
  const fenceOptions = useMemo(() => {
    const game = gameRef.current
    if (!game || !snapshot || !selectedHabitat) {
      return []
    }
    return snapshot.fence_catalog
      .filter((fence) => fence.key !== selectedHabitat.fence.key)
      .flatMap((fence) => {
        const text = callGame(() => game.evaluate_fence_upgrade(selectedHabitat.id, fence.key))
        return text === undefined
          ? []
          : [{ fence, preview: JSON.parse(text) as ConstructionPreview }]
      })
  }, [callGame, selectedHabitat, snapshot])

  const selectedGuest = useMemo(
    () => snapshot?.guests.find((guest) => guest.id === selectedGuestId) ?? null,
    [selectedGuestId, snapshot],
  )

  const selectedAnimal = useMemo(
    () => snapshot?.animals.find((animal) => animal.id === selectedAnimalId) ?? null,
    [selectedAnimalId, snapshot],
  )

  const watchedAnimal = useMemo(
    () => snapshot?.animals.find((animal) => animal.id === watchedAnimalId) ?? null,
    [watchedAnimalId, snapshot],
  )

  // Rust's verdict and effect for adopting the chosen group of each species here (#108).
  const adoptionPreviews = useMemo(() => {
    const previews = new Map<SpeciesKey, ArrivalPreview>()
    if (!snapshot || !selectedHabitat) {
      return previews
    }
    for (const offer of snapshot.species_catalog) {
      const text = callGame((game) =>
        game.evaluate_adoption(selectedHabitat.id, offer.key, adoptCount),
      )
      if (text !== undefined) {
        previews.set(offer.key, JSON.parse(text) as ArrivalPreview)
      }
    }
    return previews
  }, [adoptCount, callGame, selectedHabitat, snapshot])

  // Rust's preview of moving the selected animal to the chosen habitat (#108).
  const transferPreview = useMemo(() => {
    if (!snapshot || !selectedAnimal || transferTarget === null) {
      return null
    }
    if (transferTarget === selectedAnimal.habitat_id) {
      return null
    }
    const text = callGame((game) => game.evaluate_transfer(selectedAnimal.id, transferTarget))
    return text === undefined ? null : (JSON.parse(text) as ArrivalPreview)
  }, [callGame, selectedAnimal, snapshot, transferTarget])

  // Rust's preview of moving the selected ill animal into quarantine (#109).
  const quarantinePreview = useMemo(() => {
    if (!snapshot || !selectedAnimal) {
      return null
    }
    const { health } = selectedAnimal
    if (health.in_quarantine || (health.status !== "unwell" && health.status !== "recovering")) {
      return null
    }
    const text = callGame((game) => game.evaluate_quarantine(selectedAnimal.id))
    return text === undefined ? null : (JSON.parse(text) as QuarantinePreview)
  }, [callGame, selectedAnimal, snapshot])

  const selectedZone = useMemo(
    () => snapshot?.work_zones.find((zone) => zone.id === selectedZoneId) ?? null,
    [selectedZoneId, snapshot],
  )

  const selectedConcession = useMemo(
    () => snapshot?.concessions.find((stand) => stand.id === selectedConcessionId) ?? null,
    [selectedConcessionId, snapshot],
  )

  // A demolished, departed or deleted object leaves no stale selection behind.
  useEffect(() => {
    if (!snapshot) {
      return
    }
    if (selectedHabitatId !== null && !snapshot.habitats.some((h) => h.id === selectedHabitatId)) {
      setSelectedHabitatId(null)
    }
    if (selectedGuestId !== null && !snapshot.guests.some((g) => g.id === selectedGuestId)) {
      setSelectedGuestId(null)
    }
    if (selectedAnimalId !== null && !snapshot.animals.some((a) => a.id === selectedAnimalId)) {
      setSelectedAnimalId(null)
    }
    if (watchedAnimalId !== null && !snapshot.animals.some((a) => a.id === watchedAnimalId)) {
      setWatchedAnimalId(null)
    }
    if (selectedZoneId !== null && !snapshot.work_zones.some((z) => z.id === selectedZoneId)) {
      setSelectedZoneId(null)
    }
    if (
      selectedConcessionId !== null &&
      !snapshot.concessions.some((stand) => stand.id === selectedConcessionId)
    ) {
      setSelectedConcessionId(null)
    }
  }, [
    selectedAnimalId,
    selectedConcessionId,
    selectedGuestId,
    selectedHabitatId,
    selectedZoneId,
    snapshot,
    watchedAnimalId,
  ])

  // The active overlay is re-read from zoo-core with each snapshot, never per rendered frame.
  const overlayView = useMemo<OverlayView | null>(() => {
    if (!snapshot || overlayKind === null) {
      return null
    }
    const text = callGame((game) => game.overlay_json(overlayKind))
    return text === undefined ? null : (JSON.parse(text) as OverlayView | null)
  }, [callGame, overlayKind, snapshot])

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

  // A drawn fence, or (without a gesture) the typed habitat size anchored at the hovered tile.
  const fenceCorners = useMemo((): [Point, Point] | null => {
    if (tool !== "habitat" || habitatShape !== "rect") {
      return null
    }
    if (fenceStart && fenceEnd) {
      return [fenceStart, fenceEnd]
    }
    if (hoveredTile) {
      return [
        hoveredTile,
        {
          x: hoveredTile.x + habitatSize.width - 1,
          y: hoveredTile.y + habitatSize.height - 1,
        },
      ]
    }
    return null
  }, [fenceEnd, fenceStart, habitatShape, habitatSize, hoveredTile, tool])

  // Rectangles, outlines and reshapes all preview through Rust's enclosure plan.
  const placement = useMemo(() => {
    const game = gameRef.current
    if (!game || !snapshot) {
      return null
    }
    let text: string | undefined
    if (outlineMode) {
      if (outline.length === 0) {
        return null
      }
      const points = Uint32Array.from(outline.flatMap((corner) => [corner.x, corner.y]))
      if (tool !== "reshape") {
        text = callGame(() => game.evaluate_habitat_outline(points))
      } else if (selectedHabitatId !== null) {
        text = callGame(() => game.evaluate_habitat_reshape(selectedHabitatId, points))
      }
    } else if (fenceCorners) {
      const [a, b] = fenceCorners
      text = callGame(() => game.evaluate_habitat_rect(a.x, a.y, b.x, b.y))
    }
    return text === undefined ? null : (JSON.parse(text) as PlacementEvaluation)
  }, [callGame, fenceCorners, outline, outlineMode, selectedHabitatId, snapshot, tool])

  // Path strokes, facilities and demolition preview through the same Rust plan the commit runs.
  const constructionPreview = useMemo(() => {
    const game = gameRef.current
    if (!game || !snapshot) {
      return null
    }
    let text: string | undefined
    if (plazaMode) {
      const a = fenceStart ?? hoveredTile
      const b = fenceEnd ?? hoveredTile
      if (!a || !b) {
        return null
      }
      text = callGame(() => game.evaluate_plaza(a.x, a.y, b.x, b.y, pathBrush.access))
    } else if (tool === "path") {
      const tiles = stroke.length > 0 || !hoveredTile ? stroke : [hoveredTile]
      if (tiles.length === 0) {
        return null
      }
      const points = Uint32Array.from(tiles.flatMap((tile) => [tile.x, tile.y]))
      text = callGame(() => game.evaluate_path_stroke(points, pathBrush.width, pathBrush.access))
    } else if (tool.startsWith("facility:") && hoveredTile) {
      text = callGame(() => game.evaluate_concession(hoveredTile.x, hoveredTile.y, tool.slice(9)))
    } else if (tool === "bulldoze" && hoveredTile) {
      text = callGame(() => game.evaluate_demolition(hoveredTile.x, hoveredTile.y))
    }
    return text === undefined ? null : (JSON.parse(text) as ConstructionPreview)
  }, [
    callGame,
    fenceEnd,
    fenceStart,
    hoveredTile,
    pathBrush.access,
    pathBrush.width,
    plazaMode,
    snapshot,
    stroke,
    tool,
  ])

  // Habitat design (#97): the stroke (or hovered tile) previewed through the same Rust plan
  // the terrain, elevation or furnishing command runs on release.
  const environmentPreview = useMemo(() => {
    const game = gameRef.current
    if (!game || !snapshot || tool !== "terrain" || selectedHabitatId === null) {
      return null
    }
    const tiles = stroke.length > 0 || !hoveredTile ? stroke : [hoveredTile]
    const last = tiles.at(-1)
    if (!last) {
      return null
    }
    const points = Uint32Array.from(tiles.flatMap((tile) => [tile.x, tile.y]))
    let text: string | undefined
    if (brush.kind === "surface") {
      text = callGame(() => game.evaluate_habitat_terrain(selectedHabitatId, points, brush.surface))
    } else if (brush.kind === "height") {
      text = callGame(() =>
        game.evaluate_habitat_elevation(selectedHabitatId, points, brush.height),
      )
    } else {
      text = callGame(() =>
        game.evaluate_habitat_object(selectedHabitatId, brush.key, last.x, last.y),
      )
    }
    return text === undefined ? null : (JSON.parse(text) as EnvironmentPreview)
  }, [brush, callGame, hoveredTile, selectedHabitatId, snapshot, stroke, tool])

  // Land (#99): bought parcels leave the selection; Rust quotes the rest as one purchase.
  useEffect(() => {
    if (!snapshot) {
      return
    }
    setLandSelection((current) => {
      const next = current.filter((id) =>
        snapshot.land.parcels.some((parcel) => parcel.id === id && !parcel.owned),
      )
      return next.length === current.length ? current : next
    })
  }, [snapshot])

  const landQuote = useMemo<LandQuote | null>(() => {
    if (!snapshot || landSelection.length === 0) {
      return null
    }
    const text = callGame((game) => game.evaluate_land_purchase(landSelection))
    return text === undefined ? null : (JSON.parse(text) as LandQuote)
  }, [callGame, landSelection, snapshot])

  // The expansion plan is re-evaluated by Rust against every snapshot; nothing is committed.
  const planEvaluations = useMemo<PlanEvaluation[]>(() => {
    if (!snapshot) {
      return []
    }
    return plan.flatMap((item): PlanEvaluation[] => {
      const text = callGame((game) => {
        switch (item.kind) {
          case "path":
            return game.evaluate_path_stroke(Uint32Array.from(item.points), item.width, item.access)
          case "plaza":
            return game.evaluate_plaza(item.a.x, item.a.y, item.b.x, item.b.y, item.access)
          case "habitat":
            return game.evaluate_habitat_rect(item.a.x, item.a.y, item.b.x, item.b.y)
          case "outline":
            return game.evaluate_habitat_outline(Uint32Array.from(item.points))
        }
      })
      if (text === undefined) {
        return []
      }
      if (item.kind === "habitat" || item.kind === "outline") {
        const evaluation = JSON.parse(text) as PlacementEvaluation
        return [
          {
            item,
            ok: evaluation.ok,
            message: evaluation.message,
            cost_cents: evaluation.cost_cents,
            tiles: evaluation.occupied_tiles,
            land_parcels: evaluation.land_parcels,
          },
        ]
      }
      const preview = JSON.parse(text) as ConstructionPreview
      return [
        {
          item,
          ok: preview.ok && preview.changes,
          message: preview.ok && !preview.changes ? "Already built" : preview.message,
          cost_cents: preview.cost_cents,
          tiles: [...preview.tiles, ...preview.blocked_tiles],
          land_parcels: preview.land_parcels,
        },
      ]
    })
  }, [callGame, plan, snapshot])

  const addToPlan = (item: PlanItem) => {
    setPlan((current) => [...current, item])
    inform("Added to the plan · nothing was built or spent.")
  }

  const nextPlanId = () => {
    planIdRef.current += 1
    return planIdRef.current
  }

  const buildPlanItem = (item: PlanItem) => {
    const game = gameRef.current
    if (!game) {
      return
    }
    const result = perform(() => {
      switch (item.kind) {
        case "path":
          return game.build_path_stroke(Uint32Array.from(item.points), item.width, item.access)
        case "plaza":
          return game.build_plaza(item.a.x, item.a.y, item.b.x, item.b.y, item.access)
        case "habitat":
          return game.place_habitat_rect(item.a.x, item.a.y, item.b.x, item.b.y)
        case "outline":
          return game.place_habitat_outline(Uint32Array.from(item.points))
      }
    })
    if (result.ok) {
      setPlan((current) => current.filter((entry) => entry.id !== item.id))
    }
  }

  const buyLand = () => {
    const game = gameRef.current
    if (!game || !landQuote) {
      return
    }
    const result = perform(() => game.buy_land(landSelection, landQuote.quote))
    if (result.ok) {
      setLandSelection([])
    }
  }

  const toggleParcel = (id: string) =>
    setLandSelection((current) =>
      current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id],
    )

  // The scene and tooltip show an environment plan like any other construction plan.
  const scenePreview = useMemo<ConstructionPreview | null>(() => {
    if (constructionPreview || !environmentPreview) {
      return constructionPreview
    }
    return {
      ok: environmentPreview.ok,
      changes: environmentPreview.changed_tiles.length > 0,
      message: environmentPreview.message,
      action: "fence",
      cost_cents: environmentPreview.cost_cents,
      tiles: environmentPreview.changed_tiles,
      blocked_tiles: environmentPreview.blocking_tiles,
      affected: [],
      undoable: false,
      land_parcels: [],
    }
  }, [constructionPreview, environmentPreview])

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
    setTool((current) =>
      current === "gate" || current === "reshape" || current === "terrain" ? "select" : current,
    )
  }, [selectedHabitatId])

  const gatePreview = gateCandidate ?? (gateHover?.segment ? gateHover : null)

  const ghostTiles = useMemo(() => {
    if (outlineMode) {
      return placement?.occupied_tiles ?? []
    }
    return snapshot && fenceCorners ? previewTiles(fenceCorners[0], fenceCorners[1], snapshot) : []
  }, [fenceCorners, outlineMode, placement, snapshot?.width, snapshot?.height])

  const zonesVisible = tool === "zone" || selectedZoneId !== null

  const overlay = useMemo<SceneOverlay>(
    () => ({
      tool,
      hoveredTile,
      ghostTiles,
      outline,
      placement,
      construction: scenePreview,
      selectedHabitatId,
      selectedGuestId,
      selectedAnimalId: selectedAnimalId ?? watchedAnimalId,
      selectedDepot,
      gatePreview,
      zonesVisible,
      selectedZoneId,
      zonePreview,
      management: overlayView,
      land: snapshot
        ? { parcels: snapshot.land.parcels, selected: landSelection, active: tool === "land" }
        : null,
      plan: planEvaluations.map((entry) => ({
        key: `${entry.item.id}`,
        tiles: entry.tiles,
        ok: entry.ok,
      })),
    }),
    [
      landSelection,
      planEvaluations,
      snapshot,
      scenePreview,
      gatePreview,
      ghostTiles,
      hoveredTile,
      overlayView,
      outline,
      placement,
      selectedAnimalId,
      watchedAnimalId,
      selectedDepot,
      selectedGuestId,
      selectedHabitatId,
      selectedZoneId,
      tool,
      zonePreview,
      zonesVisible,
    ],
  )

  const extendStroke = (tile: Point) => {
    const last = strokeRef.current.at(-1)
    if (last && last.x === tile.x && last.y === tile.y) {
      return
    }
    strokeRef.current = [...strokeRef.current, tile]
    setStroke(strokeRef.current)
  }

  const extendOutline = (corner: Point) => {
    const last = outlineRef.current.at(-1)
    if (last && last.x === corner.x && last.y === corner.y) {
      return
    }
    outlineRef.current = [...outlineRef.current, corner]
    setOutline(outlineRef.current)
  }

  // An outline closes when it returns to its first corner; Rust decides whether it is valid.
  const outlineClosed = (corners: Point[]) => {
    const first = corners[0]
    const last = corners.at(-1)
    return corners.length >= 4 && !!first && !!last && first.x === last.x && first.y === last.y
  }

  const commitOutline = () => {
    const game = gameRef.current
    const points = Uint32Array.from(outlineRef.current.flatMap((corner) => [corner.x, corner.y]))
    clearOutline()
    if (!game) {
      return
    }
    if (tool === "reshape" && selectedHabitatId !== null) {
      const result = perform(() => game.reshape_habitat(selectedHabitatId, points))
      if (result.ok) {
        setTool("select")
      }
    } else {
      if (planning) {
        addToPlan({ id: nextPlanId(), kind: "outline", points: [...points] })
        return
      }
      perform(() => game.place_habitat_outline(points))
    }
  }

  const cancelOutline = () => {
    clearOutline()
    inform("Fence outline cleared · nothing was built.")
  }

  const onTileDown = (tile: Point) => {
    if (outlineMode) {
      extendOutline(tile)
    } else if ((tool === "path" && !plazaMode) || tool === "terrain") {
      strokeRef.current = []
      extendStroke(tile)
    } else if (tool === "habitat" || tool === "zone" || plazaMode) {
      fenceStartRef.current = tile
      fenceEndRef.current = tile
      setFenceStart(tile)
      setFenceEnd(tile)
    }
  }

  const onTileDrag = (tile: Point) => {
    if (outlineMode) {
      extendOutline(tile)
    } else if (plazaMode && fenceStartRef.current) {
      fenceEndRef.current = tile
      setFenceEnd(tile)
    } else if (tool === "path") {
      extendStroke(tile)
    } else if (tool === "terrain") {
      // A furnishing goes on one tile: the stroke keeps only the latest.
      if (brush.kind === "object") {
        strokeRef.current = []
      }
      extendStroke(tile)
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

  // One gesture commits at most one construction transaction; a cancelled gesture commits none.
  const onToolGestureEnd = (commit: boolean) => {
    const game = gameRef.current
    if (outlineMode) {
      // Esc or a lost pointer clears the outline; a release keeps it until it closes.
      if (!commit) {
        cancelOutline()
      } else if (outlineClosed(outlineRef.current)) {
        commitOutline()
      } else {
        inform("Keep drawing: tap or drag back to the first corner to close the fence.")
      }
      return
    }
    if (tool === "zone") {
      finishZoneGesture(commit)
      return
    }
    if (plazaMode) {
      const start = fenceStartRef.current
      const end = fenceEndRef.current
      clearFenceGesture()
      if (commit && game && start && end) {
        if (planning) {
          addToPlan({ id: nextPlanId(), kind: "plaza", a: start, b: end, access: pathBrush.access })
          return
        }
        perform(() => game.build_plaza(start.x, start.y, end.x, end.y, pathBrush.access))
      } else if (!commit) {
        inform("Plaza cancelled · nothing was built.")
      }
      return
    }
    if (tool === "path") {
      const points = Uint32Array.from(strokeRef.current.flatMap((tile) => [tile.x, tile.y]))
      clearStroke()
      if (commit && game && points.length > 0) {
        if (planning) {
          addToPlan({
            id: nextPlanId(),
            kind: "path",
            points: [...points],
            width: pathBrush.width,
            access: pathBrush.access,
          })
          return
        }
        perform(() => game.build_path_stroke(points, pathBrush.width, pathBrush.access))
      } else if (!commit) {
        inform("Path stroke cancelled · nothing was built.")
      }
      return
    }
    if (tool === "terrain") {
      const tiles = strokeRef.current
      clearStroke()
      const last = tiles.at(-1)
      if (!commit) {
        inform("Habitat edit cancelled · nothing changed.")
      } else if (game && last && selectedHabitatId !== null) {
        const points = Uint32Array.from(tiles.flatMap((tile) => [tile.x, tile.y]))
        if (brush.kind === "surface") {
          perform(() => game.paint_habitat_terrain(selectedHabitatId, points, brush.surface))
        } else if (brush.kind === "height") {
          perform(() => game.set_habitat_elevation(selectedHabitatId, points, brush.height))
        } else {
          perform(() => game.place_habitat_object(selectedHabitatId, brush.key, last.x, last.y))
        }
      }
      return
    }
    if (tool !== "habitat") {
      return
    }
    const start = fenceStartRef.current
    const end = fenceEndRef.current
    clearFenceGesture()
    if (!commit) {
      if (start) {
        inform("Fence cancelled · nothing was built.")
      }
      return
    }
    if (game && start && end) {
      // A tap places the typed size with its north-west corner on the tapped tile.
      const tap = start.x === end.x && start.y === end.y
      const far = tap
        ? { x: start.x + habitatSize.width - 1, y: start.y + habitatSize.height - 1 }
        : end
      if (planning) {
        addToPlan({ id: nextPlanId(), kind: "habitat", a: start, b: far })
        return
      }
      perform(() => game.place_habitat_rect(start.x, start.y, far.x, far.y))
    }
  }

  const undoConstruction = () => {
    const game = gameRef.current
    if (game) {
      perform(() => game.undo_construction())
    }
  }

  const redoConstruction = () => {
    const game = gameRef.current
    if (game) {
      perform(() => game.redo_construction())
    }
  }

  const undoRef = useRef(undoConstruction)
  const redoRef = useRef(redoConstruction)
  undoRef.current = undoConstruction
  redoRef.current = redoConstruction
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target
      if (
        !(event.ctrlKey || event.metaKey) ||
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement
      ) {
        return
      }
      const key = event.key.toLowerCase()
      if (key === "z" && !event.shiftKey) {
        event.preventDefault()
        undoRef.current()
      } else if ((key === "z" && event.shiftKey) || key === "y") {
        event.preventDefault()
        redoRef.current()
      }
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [])

  const inform = (text: string) => {
    setMessage(text)
    setMessageKind("info")
  }

  const selectHabitat = (habitatId: number | null) => {
    setSelectedGuestId(null)
    setSelectedAnimalId(null)
    setSelectedDepot(false)
    setSelectedZoneId(null)
    setSelectedConcessionId(null)
    setSelectedHabitatId(habitatId)
  }

  const selectZone = (zoneId: number | null) => {
    setSelectedGuestId(null)
    setSelectedAnimalId(null)
    setSelectedDepot(false)
    setSelectedHabitatId(null)
    setSelectedConcessionId(null)
    setSelectedZoneId(zoneId)
  }

  const selectConcession = (standId: number) => {
    setSelectedGuestId(null)
    setSelectedAnimalId(null)
    setSelectedDepot(false)
    setSelectedHabitatId(null)
    setSelectedZoneId(null)
    setSelectedConcessionId(standId)
    describeStand(standId)
  }

  /** Inspects one individual animal by its stable ID (#96). */
  const selectAnimal = (animalId: number) => {
    setSelectedGuestId(null)
    setSelectedHabitatId(null)
    setSelectedZoneId(null)
    setSelectedDepot(false)
    setSelectedConcessionId(null)
    setSelectedAnimalId(animalId)
    setTransferTarget(null)
    const picked = snapshot?.animals.find((animal) => animal.id === animalId)
    setAnimalName(picked?.name ?? "")
    if (picked) {
      inform(`${picked.name} selected · ${picked.activity}`)
    }
  }

  /**
   * Picking an animal watches it first (#141): the camera follows the individual and a compact
   * label shows zoo-core's name and activity, with the detailed inspector one deliberate step away.
   */
  const watchAnimal = (animalId: number) => {
    setSelectedGuestId(null)
    setSelectedHabitatId(null)
    setSelectedZoneId(null)
    setSelectedDepot(false)
    setSelectedConcessionId(null)
    setSelectedAnimalId(null)
    setWatchedAnimalId(animalId)
    const picked = snapshot?.animals.find((animal) => animal.id === animalId)
    if (picked) {
      inform(`Watching ${picked.name}`)
    }
  }

  const renameAnimal = () => {
    const game = gameRef.current
    if (!game || selectedAnimalId === null) {
      return
    }
    perform(() => game.rename_animal(selectedAnimalId, animalName))
  }

  const selectGuest = (guestId: number) => {
    setSelectedHabitatId(null)
    setSelectedZoneId(null)
    setSelectedDepot(false)
    setSelectedAnimalId(null)
    setSelectedConcessionId(null)
    setSelectedGuestId(guestId)
  }

  const isPhone = () => window.matchMedia("(max-width: 900px)").matches

  /** Focuses an alert's, overlay item's or table row's object and opens its inspector. */
  const jumpTo = (target: JumpTarget, focus: Point) => {
    setTool("select")
    switch (target.kind) {
      case "habitat":
        selectHabitat(target.id)
        break
      case "concession":
        selectConcession(target.id)
        break
      case "depot":
        selectDepot()
        break
      case "guest":
        selectGuest(target.id)
        break
      case "zone":
        selectZone(target.id)
        break
      case "keeper": {
        const keeper = snapshot?.animal_care_depot.keepers.find((entry) => entry.id === target.id)
        if (keeper?.zone_id != null) {
          selectZone(keeper.zone_id)
        } else {
          selectDepot()
        }
        break
      }
      case "animal":
        selectAnimal(target.id)
        break
      case "finances":
        setWorkspaceTab("finances")
        return
      case "paths":
        selectHabitat(null)
        inform(`Showing path tile ${focus.x},${focus.y}`)
        break
    }
    requestCamera(focus)
    // On a phone the workspace drawer would cover the inspector it just opened.
    if (isPhone()) {
      setWorkspaceTab(null)
    }
  }

  const alertActionLabel = (alert: Alert): string | null => {
    if (!snapshot) {
      return null
    }
    const depot = snapshot.animal_care_depot
    switch (alert.action) {
      case "buy_feed":
        return `Buy ${depot.feed_batch_crates} feed · ${money(depot.feed_batch_cost_cents)}`
      case "hire_keeper":
        return `Hire keeper · ${money(depot.keeper_hire_cost_cents)}`
      case "hire_janitor":
        return `Hire janitor · ${money(depot.janitor_hire_cost_cents)}`
      case "hire_mechanic":
        return `Hire mechanic · ${money(depot.mechanic_hire_cost_cents)}`
      case "build_path":
        return "Build path here"
      case "draw_zone":
        return "Draw a work zone"
      case "assign_keeper":
        return "Assign a keeper"
      case "adopt_animals":
        return "Adopt animals"
      case "inspect_habitat":
        return "Open habitat"
      case "inspect_facility":
        return "Open facility"
      case "build_toilet":
      case "build_bench":
      case "build_water_pump":
      case "build_climate_plant": {
        const service = BUILD_ACTION_SERVICE[alert.action]
        const offer = snapshot.facility_catalog.find((entry) => entry.service === service)
        return offer ? `Build ${offer.label} · ${money(offer.build_cost_cents)}` : null
      }
      case "hire_vet":
        return `Hire vet · ${money(depot.vet_hire_cost_cents)}`
      case "buy_medicine":
        return `Buy ${depot.medicine_batch_kits} medicine · ${money(depot.medicine_batch_cost_cents)}`
      case "inspect_animal":
        return "Open animal"
      case "open_finances":
        return "Open finances"
      case "add_shelter":
        return "Add shelter"
    }
  }

  /** Runs an alert's suggested fix: a command, or the tool and inspector that make it. */
  const runAlertAction = (alert: Alert) => {
    const game = gameRef.current
    if (!game || !snapshot) {
      return
    }
    switch (alert.action) {
      case "buy_feed":
        perform(() => game.buy_animal_feed())
        return
      case "hire_keeper":
        perform(() => game.hire_keeper())
        return
      case "hire_janitor":
        perform(() => game.hire_janitor())
        return
      case "hire_mechanic":
        perform(() => game.hire_mechanic())
        return
      case "hire_vet":
        perform(() => game.hire_vet())
        return
      case "buy_medicine":
        perform(() => game.buy_medicine())
        return
      case "open_finances":
        setWorkspaceTab("finances")
        return
      case "add_shelter": {
        const { subject } = alert
        if (subject.kind === "habitat") {
          perform(() => game.add_shelter(subject.id))
        }
        return
      }
      case "inspect_animal":
        jumpTo(alert.subject, alert.focus)
        return
      case "build_path":
        jumpTo(alert.subject, alert.focus)
        setTool("path")
        setWorkspaceTab(null)
        inform("Path tool: drag from a connected path to the highlighted problem.")
        return
      case "draw_zone":
        jumpTo(alert.subject, alert.focus)
        setTool("zone")
        setWorkspaceTab(null)
        return
      case "assign_keeper": {
        const { subject } = alert
        const habitat =
          subject.kind === "habitat"
            ? snapshot.habitats.find((entry) => entry.id === subject.id)
            : undefined
        const zoneId = habitat?.work_zone_id ?? snapshot.work_zones[0]?.id ?? null
        jumpTo(zoneId === null ? alert.subject : { kind: "zone", id: zoneId }, alert.focus)
        return
      }
      case "adopt_animals":
      case "inspect_habitat":
      case "inspect_facility":
        jumpTo(alert.subject, alert.focus)
        return
      case "build_toilet":
      case "build_bench":
      case "build_water_pump":
      case "build_climate_plant": {
        const service = BUILD_ACTION_SERVICE[alert.action]
        const offer = snapshot.facility_catalog.find((entry) => entry.service === service)
        if (!offer) {
          return
        }
        jumpTo(alert.subject, alert.focus)
        setTool(`facility:${offer.key}`)
        setWorkspaceTab(null)
        inform(
          offer.utility
            ? `${offer.label}: place it on grass beside a path that reaches the habitat's service gate.`
            : `${offer.label}: place it on grass beside a path where guests walk.`,
        )
      }
    }
  }

  const openWorkspace = (tab: WorkspaceTab) =>
    setWorkspaceTab((current) => (current === tab ? null : tab))

  const selectDepot = () => {
    setSelectedGuestId(null)
    setSelectedAnimalId(null)
    setSelectedHabitatId(null)
    setSelectedZoneId(null)
    setSelectedConcessionId(null)
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

  /** The one tool or object the guide's next step points at, if the park has it. */
  const tutorialFocus = (): { label: string; run: () => void } | null => {
    const tutorial = snapshot?.tutorial
    if (!snapshot || !tutorial || tutorial.status !== "active" || tutorial.tool === null) {
      return null
    }
    const habitat = snapshot.habitats.find((entry) => entry.id === tutorial.habitat_id)
    switch (tutorial.tool) {
      case "path":
        return { label: "Choose Path", run: () => setTool("path") }
      case "habitat":
        return { label: "Draw habitat", run: () => setTool("habitat") }
      case "gate":
        if (!habitat) {
          return { label: "Choose Path", run: () => setTool("path") }
        }
        // Selecting another habitat ends a gate move, so open the habitat first.
        if (selectedHabitatId !== habitat.id) {
          return {
            label: `Show habitat #${habitat.id}`,
            run: () => {
              setTool("select")
              selectHabitat(habitat.id)
            },
          }
        }
        return tool === "gate"
          ? null
          : { label: `Move habitat #${habitat.id}'s gate`, run: startGateMove }
      case "zone":
        return { label: "Choose Zones", run: () => setTool("zone") }
      case "depot": {
        // Staffing: once a keeper is hired and the gate is zoned, the zone inspector assigns.
        const zoneId = habitat?.work_zone_id ?? null
        if (
          tutorial.next === "staff-zone" &&
          zoneId !== null &&
          snapshot.animal_care_depot.keepers.some((worker) => worker.role === "keeper")
        ) {
          return {
            label: `Open zone #${zoneId}`,
            run: () => {
              setTool("select")
              selectZone(zoneId)
            },
          }
        }
        return {
          label: "Open depot",
          run: () => {
            setTool("select")
            selectDepot()
          },
        }
      }
      case "habitat_inspector":
        return habitat
          ? {
              label: `Show habitat #${habitat.id}`,
              run: () => {
                setTool("select")
                selectHabitat(habitat.id)
              },
            }
          : { label: "Draw habitat", run: () => setTool("habitat") }
      case "concession": {
        const offer = snapshot.facility_catalog[0]
        return offer
          ? { label: `Choose ${offer.label} stand`, run: () => setTool(`facility:${offer.key}`) }
          : null
      }
      case "speed":
        return speed === 0 || speed === 1
          ? { label: "Speed up time", run: () => setSpeed(4) }
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
      `${label} #${stand.id} · ${stand.condition}% condition · ${stand.service_state} · ${stand.maintenance_status}`,
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
      if (pick.kind === "animal") {
        watchAnimal(pick.id)
        return
      }
      // Choosing anything else stops watching and returns to ordinary park navigation.
      setWatchedAnimalId(null)
      switch (pick.kind) {
        case "guest":
          selectGuest(pick.id)
          return
        case "depot":
          selectDepot()
          return
        case "concession":
          selectConcession(pick.id)
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
            selectConcession(tile.concession_id)
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

    if (tool === "land") {
      const parcel = target ? parcelAt(snapshot.land, target) : null
      if (!parcel) {
        inform("This tile is already the park's own land.")
      } else if (parcel.owned) {
        inform(`${parcel.title} is owned land.`)
      } else {
        toggleParcel(parcel.id)
        inform(
          `${parcel.title} · ${money(parcel.price_cents)} · ${parcel.restriction ?? parcel.summary}`,
        )
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
    perform(() =>
      adoptCount === 1
        ? game.adopt(selectedHabitatId, species)
        : game.adopt_group(selectedHabitatId, species, adoptCount),
    )
  }

  const transferAnimal = () => {
    const game = gameRef.current
    if (!game || selectedAnimalId === null || transferTarget === null) {
      return
    }
    if (perform(() => game.transfer_animal(selectedAnimalId, transferTarget)).ok) {
      setTransferTarget(null)
    }
  }

  const quarantineAnimal = () => {
    const game = gameRef.current
    if (!game || selectedAnimalId === null) {
      return
    }
    perform(() => game.quarantine_animal(selectedAnimalId))
  }

  /** A treatment's suggested fix, run like the matching alert action (#109). */
  const healthFix = (action: AlertAction): Alert | null =>
    selectedAnimal
      ? {
          code: "animal_unwell",
          severity: "warning",
          subject: { kind: "animal", id: selectedAnimal.id },
          focus: { x: selectedAnimal.x, y: selectedAnimal.y },
          message: "",
          action,
          first_minute: 0,
          last_minute: 0,
        }
      : null

  const rehomeAnimal = () => {
    const game = gameRef.current
    if (!game || selectedAnimalId === null) {
      return
    }
    if (perform(() => game.rehome_animal(selectedAnimalId)).ok) {
      setSelectedAnimalId(null)
    }
  }

  const upgradeFence = (fence: string) => {
    const game = gameRef.current
    if (!game || selectedHabitatId === null) {
      return
    }
    perform(() => game.upgrade_fence(selectedHabitatId, fence))
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

  const hireVet = () => {
    const game = gameRef.current
    if (!game) {
      return
    }
    perform(() => game.hire_vet())
  }

  const buyMedicine = () => {
    const game = gameRef.current
    if (!game) {
      return
    }
    perform(() => game.buy_medicine())
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

  /** The stand inspector's commands (#102); zoo-core validates each one. */
  const standCommands = (standId: number) => {
    const send = (command: (game: ZooGame) => string) => {
      const game = gameRef.current
      if (game) {
        perform(() => command(game))
      }
    }
    return {
      setVendors: (vendors: number) => send((game) => game.set_stand_vendors(standId, vendors)),
      setOpen: (open: boolean) => send((game) => game.set_stand_open(standId, open)),
      restock: () => send((game) => game.restock_stand(standId)),
      setAutoRestock: (auto: boolean) => send((game) => game.set_stand_auto_restock(standId, auto)),
      setPrice: (cents: number) => send((game) => game.set_stand_price(standId, cents)),
    }
  }

  /** The pricing panel's commands (#106); zoo-core validates each one. */
  const sendPricing = (command: (game: ZooGame) => string) => {
    const game = gameRef.current
    if (game) {
      perform(() => command(game))
    }
  }
  const pricingCommands: PricingCommands = {
    setAdmission: (cents) => sendPricing((game) => game.set_admission_price(cents)),
    setParkOpen: (open) => sendPricing((game) => game.set_park_open(open)),
    setStandPrice: (standId, cents) => sendPricing((game) => game.set_stand_price(standId, cents)),
    startCampaign: () => sendPricing((game) => game.start_marketing_campaign()),
    evaluateAdmission: (cents) => {
      const text = callGame((game) => game.evaluate_admission_price(cents))
      return text === undefined ? null : (JSON.parse(text) as AdmissionEvaluation)
    },
  }
  const educationCommands: EducationCommands = {
    scheduleTalk: (habitatId, start, duration, places) =>
      sendPricing((game) => game.schedule_talk(habitatId, start, duration, places)),
    cancelTalk: (habitatId) => sendPricing((game) => game.cancel_talk(habitatId)),
  }
  const financeCommands: FinanceCommands = {
    borrow: (cents) => sendPricing((game) => game.borrow_loan(cents)),
    repay: (cents) => sendPricing((game) => game.repay_loan(cents)),
    setAdmission: pricingCommands.setAdmission,
    setStandVendors: (standId, vendors) =>
      sendPricing((game) => game.set_stand_vendors(standId, vendors)),
    evaluateBudget: (spend, borrow) => {
      const text = callGame((game) => game.evaluate_budget(spend, borrow))
      return text === undefined ? null : (JSON.parse(text) as BudgetEvaluation)
    },
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
    background(newPark(snapshot?.scenario.id ?? DEFAULT_SCENARIO_ID))
  }

  const resetView = () => {
    clearStroke()
    clearFenceGesture()
    setTool("select")
    setLandSelection([])
    setPlan([])
    setPlanning(false)
    setSelectedHabitatId(null)
    setSelectedGuestId(null)
    setSelectedAnimalId(null)
    setSelectedDepot(false)
    setSelectedZoneId(null)
    setHoveredTile(null)
    requestCamera(null)
  }

  const enterPlay = () => {
    setPrompt(null)
    setMenuOpen(false)
    setScreen("play")
    return refresh()
  }

  /** A fresh park in a new library slot; existing saves are never touched. */
  const newPark = async (scenarioId: string) => {
    const text = callGame((game) => game.start_scenario(scenarioId))
    const result: ActionResult =
      text === undefined
        ? { ok: false, message: "The park simulation is stopped." }
        : (JSON.parse(text) as ActionResult)
    if (!result.ok) {
      notify(`${result.message} The current park is unchanged.`, "error")
      return null
    }
    resetView()
    diagnostics.retain()
    const next = enterPlay()
    if (next) {
      // Open on the scenario's first objective; Reset view still frames the whole park.
      requestCamera(next.scenario.opening_focus)
    }
    const slot = await library.createSlot(library.defaultName())
    const hint = next ? scenarioHint(next.scenario) : ""
    notify(
      slot
        ? `New park “${slot.name}” started. ${hint}`
        : `New park started. It cannot be stored in this browser; export it to keep it. ${hint}`,
    )
    return next
  }

  /** Replaces the active park with a stored save once zoo-core accepts it. */
  const openSave = (document: string, entry: SlotEntry | null, label: string) => {
    const text = callGame((game) => game.load_save(document))
    const result: LoadCheck =
      text === undefined
        ? { ok: false, message: "The park simulation is stopped." }
        : (JSON.parse(text) as LoadCheck)
    if (!result.ok) {
      notify(
        `${label} could not be opened: ${loadFailureMessage(result.code, result.message)} The current park is unchanged.`,
        "error",
      )
      return false
    }
    resetView()
    diagnostics.retain()
    const summary = summarizeDocument(document)
    library.adopt(entry?.slot ?? null, (summary.day - 1) * 24 * 60 + summary.minute)
    if (entry) {
      background(library.touch(entry.slot.id))
    }
    enterPlay()
    notify(`${result.message} · ${label}.`)
    return true
  }

  const loadEntry = async (entry: SlotEntry, save: SaveSummary) => {
    const label = `“${entry.slot.name}” ${save.kind === "auto" ? "autosave" : "save"}`
    try {
      openSave(await library.readSave(save.key), entry, label)
    } catch (error) {
      notify(
        `${label} could not be read: ${error instanceof Error ? error.message : String(error)}`,
        "error",
      )
    }
  }

  /** Warns before an action would replace a park with progress since its last save. */
  const guardReplace = (action: string, run: () => void) => {
    if (screen !== "play" || !library.dirty() || stoppedRef.current) {
      run()
      return
    }
    const name = library.active?.name ?? "This park"
    setPrompt({
      message: `${name} has progress that is not saved yet. ${action} replaces it in this tab; saved parks stay in the library.`,
      actions: [
        ...(library.active
          ? [
              {
                label: "Save and continue",
                run: () => {
                  background(
                    library.save("manual").then((saved) => {
                      setPrompt(null)
                      if (saved) {
                        run()
                      }
                    }),
                  )
                },
              },
            ]
          : []),
        {
          label: "Discard changes",
          run: () => {
            setPrompt(null)
            run()
          },
        },
        { label: "Cancel", run: () => setPrompt(null) },
      ],
    })
  }

  const requestNewPark = (scenarioId: string) =>
    guardReplace("A new park", () => void newPark(scenarioId))

  const requestLoad = (entry: SlotEntry, save: SaveSummary) =>
    guardReplace(`Loading “${entry.slot.name}”`, () => void loadEntry(entry, save))

  const continueLatest = () => {
    const entry = library.entries[0]
    if (entry?.saves[0]) {
      background(loadEntry(entry, entry.saves[0]))
    }
  }

  const importPark = async (file: File) => {
    const imported = await library.importFile(file)
    if (imported && !imported.stored) {
      guardReplace("Opening the imported park", () =>
        openSave(imported.document, null, "Imported park"),
      )
    }
  }

  const saveAs = async (name: string) => {
    if (await library.createSlot(name)) {
      notify(`Saved as new park “${name}”.`)
    }
  }

  const libraryPanel = (mode: "start" | "menu") => (
    <ParkLibraryPanel
      mode={mode}
      library={library}
      message={message}
      messageKind={messageKind}
      prompt={prompt}
      onContinue={continueLatest}
      scenarios={scenarios}
      initialView={mode === "menu" ? menuView : "main"}
      onNew={requestNewPark}
      onLoad={requestLoad}
      onImport={(file) => void importPark(file)}
      onSaveAs={(name) => void saveAs(name)}
      onClose={() => {
        setPrompt(null)
        setMenuOpen(false)
      }}
    />
  )

  if (!snapshot || screen === "start") {
    const starting =
      gameReady && library.ready ? libraryPanel("start") : "Preparing the park simulation…"
    return (
      <main className="loading">
        {startupFailure === null ? (
          starting
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

  const constructionTooltip =
    scenePreview && hoveredTile ? (
      <div className={scenePreview.ok ? "valid" : "invalid"}>
        <b>{constructionHeadline(scenePreview)}</b>
        <small>{constructionDetails(scenePreview)}</small>
      </div>
    ) : undefined

  const tooltip =
    zoneTooltip ??
    constructionTooltip ??
    (placement && hoveredTile && !outlineMode ? (
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
  // Keepers take zones and routine care; vets work the whole park (#109).
  const keepers = snapshot.animal_care_depot.keepers.filter((worker) => worker.role === "keeper")
  const vets = snapshot.animal_care_depot.keepers.filter((worker) => worker.role === "vet")
  const keeperChoice =
    selectedZone === null
      ? null
      : (keepers.find((keeper) => keeper.id === assignKeeperId) ??
        keepers.find((keeper) => keeper.needs_zone) ??
        keepers.find((keeper) => keeper.zone_id !== selectedZone.id) ??
        null)

  return (
    <main className="game-shell">
      <header className="topbar bevel">
        <div className="brand">
          <strong>Zoo</strong>
          <span>{library.active?.name ?? "Unsaved park"}</span>
          <small className="save-status" role="status">
            {library.status}
          </small>
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
        <button
          className={`stat conditions-button${snapshot.conditions.scheduled ? " announced" : ""}`}
          data-weather={snapshot.conditions.weather}
          data-phase={snapshot.conditions.light.phase}
          onClick={() => openWorkspace("conditions")}
          aria-label={`Weather and events: day ${snapshot.day}, ${clock(snapshot.minute_of_day)}, ${
            snapshot.conditions.season_label
          }, ${snapshot.conditions.forecast[0]?.label ?? "Fair"}${
            snapshot.conditions.scheduled
              ? `, ${snapshot.conditions.scheduled.label} announced`
              : ""
          }`}
          title={snapshot.conditions.summary}
        >
          <span>
            Day · {conditionsLabel(snapshot.conditions)}
            {snapshot.conditions.scheduled && <span aria-hidden="true"> ★</span>}
          </span>
          <strong>
            {snapshot.day} · {clock(snapshot.minute_of_day)}
          </strong>
        </button>
        <div className="manage-controls">
          <button
            className={`alerts-button${
              snapshot.alerts.some((alert) => alert.severity === "critical") ? " critical" : ""
            }${workspaceTab === "alerts" ? " active" : ""}`}
            onClick={() => openWorkspace("alerts")}
            aria-label={`Alerts: ${snapshot.alerts.length}`}
            aria-expanded={workspaceTab === "alerts"}
            title="Park alerts"
          >
            <span aria-hidden="true">⚠</span> {snapshot.alerts.length}
          </button>
          <button
            className={workspaceTab !== null && workspaceTab !== "alerts" ? "active" : ""}
            onClick={() =>
              setWorkspaceTab((current) =>
                current === null || current === "alerts" ? "summary" : null,
              )
            }
            aria-label="Management workspace"
            aria-expanded={workspaceTab !== null && workspaceTab !== "alerts"}
            title="Management: today, habitats, guests, staff, facilities, finances"
          >
            <span aria-hidden="true">▤</span>
            <span className="manage-label"> Manage</span>
          </button>
          <label className="overlay-picker">
            <span>Overlay</span>
            <select
              aria-label="Overlay"
              value={overlayKind ?? ""}
              onChange={(event) =>
                setOverlayKind(
                  event.target.value === "" ? null : (event.target.value as OverlayKind),
                )
              }
            >
              <option value="">None</option>
              {OVERLAY_KINDS.map((entry) => (
                <option key={entry.kind} value={entry.kind}>
                  {entry.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="speed-controls" aria-label="Simulation speed">
          <button
            className="park-menu-button"
            onClick={() => {
              setMenuView("main")
              setMenuOpen(true)
            }}
            title="Park menu: save, load, import, export"
            aria-label="Park menu"
            aria-haspopup="dialog"
          >
            ☰
          </button>
          <button
            className="view-reset"
            onClick={() => requestCamera(null)}
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
            display={display}
            paused={speed === 0}
            dragTool={
              tool === "path" ||
              tool === "habitat" ||
              tool === "zone" ||
              tool === "reshape" ||
              tool === "terrain"
            }
            cornerTool={outlineMode}
            camera={camera}
            follow={watchedAnimal?.id ?? null}
            onFollowEnd={() => setWatchedAnimalId(null)}
            tooltip={tooltip}
            onTileDown={onTileDown}
            onTileDrag={onTileDrag}
            onToolGestureEnd={onToolGestureEnd}
            onHover={setHoveredTile}
            onPick={onPick}
            onRendererFailure={onRendererFailure}
          />
        </Suspense>

        {watchedAnimal && (
          <AnimalWatch
            animal={watchedAnimal}
            speciesLabel={speciesLabel(watchedAnimal.species, snapshot.species_catalog)}
            detailsOpen={selectedAnimalId === watchedAnimal.id}
            onDetails={() => selectAnimal(watchedAnimal.id)}
            onStop={() => {
              setWatchedAnimalId(null)
              inform(`Stopped watching ${watchedAnimal.name}`)
            }}
          />
        )}

        {(menuOpen || prompt !== null) && libraryPanel("menu")}

        {workspaceTab !== null && (
          <ManagementWorkspace
            snapshot={snapshot}
            tab={workspaceTab}
            onTab={setWorkspaceTab}
            onClose={() => setWorkspaceTab(null)}
            onJump={jumpTo}
            onAction={runAlertAction}
            actionLabel={alertActionLabel}
            prices={<PricingPanel pricing={snapshot.pricing} commands={pricingCommands} />}
            finances={(ledger) => (
              <FinancePanel
                finance={snapshot.finance}
                cash={snapshot.cash_cents}
                commands={financeCommands}
                ledger={ledger}
              />
            )}
            education={
              <EducationPanel
                education={snapshot.education}
                habitats={snapshot.habitats
                  .filter((habitat) => !habitat.quarantine)
                  .map((habitat) => ({
                    id: habitat.id,
                    label:
                      habitat.species
                        .map(
                          (key) =>
                            snapshot.species_catalog.find((offer) => offer.key === key)?.label ??
                            key,
                        )
                        .join(" & ") || "No animals",
                    stocked: habitat.species.length > 0,
                  }))}
                commands={educationCommands}
              />
            }
            conditions={
              <ConditionsPanel
                conditions={snapshot.conditions}
                day={snapshot.day}
                onRule={(rule, enabled) =>
                  sendPricing((game) => game.set_condition_rule(rule, enabled))
                }
                display={display}
                onDisplay={changeDisplay}
              />
            }
          />
        )}

        {overlayView && (
          <OverlayLegend view={overlayView} onJump={jumpTo} onClose={() => setOverlayKind(null)} />
        )}

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
          className={`side-panel bevel${tool === "gate" || tool === "reshape" ? " gate-mode" : ""}${
            tool === "zone" && zoneEdit !== null ? " zone-edit-mode" : ""
          }${
            tool === "terrain" ? " design-mode" : ""
          }${tool === "land" ? " land-mode" : ""}${workspaceTab !== null ? " behind-workspace" : ""}`}
        >
          {tool === "land" && (
            <LandPanel
              land={snapshot.land}
              selection={landSelection}
              quote={landQuote}
              onToggle={toggleParcel}
              onClear={() => {
                setLandSelection([])
                inform("Selection cleared · nothing was bought.")
              }}
              onBuy={buyLand}
              onShow={(parcel) => requestCamera(parcelCenter(parcel))}
              onClose={() => setTool("select")}
            />
          )}
          {plan.length > 0 && (
            <PlanPanel
              plan={planEvaluations}
              land={snapshot.land}
              planning={planning}
              onBuild={buildPlanItem}
              onRemove={(item) => setPlan((current) => current.filter((entry) => entry !== item))}
              onClear={() => {
                setPlan([])
                inform("Plan cleared · nothing was built.")
              }}
            />
          )}
          {snapshot.scenario.objectives.length > 0 &&
            (snapshot.tutorial.status === "active" || snapshot.tutorial.status === "off") && (
              <ObjectiveChecklist
                scenario={snapshot.scenario}
                tutorial={snapshot.tutorial}
                open={tutorialOpen}
                onToggle={setTutorialOpen}
                focusLabel={focus?.label ?? null}
                onFocus={() => focus?.run()}
                onShow={requestCamera}
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
                    <dt>Medicine</dt>
                    <dd>{snapshot.animal_care_depot.medicine_kits} kits</dd>
                  </div>
                  <div>
                    <dt>Vets</dt>
                    <dd>{vets.length}</dd>
                  </div>
                  <div>
                    <dt>Keepers</dt>
                    <dd>{keepers.length}</dd>
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
                <button className="shop-row" onClick={buyMedicine}>
                  <span>
                    <b>Buy {snapshot.animal_care_depot.medicine_batch_kits} medicine kits</b>
                    <small>
                      Vets carry one kit to each unwell animal ·{" "}
                      {snapshot.animal_care_depot.medicine_kits} on the shelf
                    </small>
                  </span>
                  <strong>{money(snapshot.animal_care_depot.medicine_batch_cost_cents)}</strong>
                </button>
                <button className="shop-row" onClick={hireVet}>
                  <span>
                    <b>Hire vet</b>
                    <small>
                      Vets treat unwell animals anywhere in the park ·{" "}
                      {money(snapshot.animal_care_depot.vet_hourly_wage_cents)}/hr
                    </small>
                  </span>
                  <strong>{money(snapshot.animal_care_depot.vet_hire_cost_cents)}</strong>
                </button>
                <h3>Veterinary care</h3>
                {vets.length === 0 ? (
                  <div className="guest-thought">
                    No vets hired. Unwell animals wait until a vet can treat them.
                  </div>
                ) : (
                  vets.map((vet) => (
                    <dl key={vet.id} data-vet-id={vet.id}>
                      <div>
                        <dt>Vet #{vet.id}</dt>
                        <dd>{vet.status}</dd>
                      </div>
                    </dl>
                  ))
                )}
                <h3>Keeper schedule</h3>
                {keepers.length === 0 ? (
                  <div className="guest-thought">No keepers hired yet.</div>
                ) : (
                  keepers.map((keeper) => (
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
                {keepers.length === 0 ? (
                  <div className="guest-thought">Hire keepers at the central operations depot.</div>
                ) : (
                  <>
                    <label className="zone-keeper-select">
                      <span>Keeper</span>
                      <select
                        value={keeperChoice?.id ?? ""}
                        onChange={(event) => setAssignKeeperId(Number(event.target.value))}
                      >
                        {keepers.map((keeper) => (
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
          {!selectedDepot && selectedAnimal && (
            <AnimalInspector
              animal={selectedAnimal}
              habitat={snapshot.habitats.find((h) => h.id === selectedAnimal.habitat_id) ?? null}
              speciesLabel={speciesLabel(selectedAnimal.species, snapshot.species_catalog)}
              name={animalName}
              onNameChange={setAnimalName}
              onRename={renameAnimal}
              onLocate={() => {
                // Locating an individual watches it (#141): the camera eases onto it and follows.
                setWatchedAnimalId(selectedAnimal.id)
                inform(`Camera moved to ${selectedAnimal.name}`)
              }}
              onShowHabitat={() => selectHabitat(selectedAnimal.habitat_id)}
              onClose={() => setSelectedAnimalId(null)}
              health={
                <AnimalHealthPanel
                  animal={selectedAnimal}
                  quarantine={quarantinePreview}
                  fixLabel={(action) => {
                    const alert = healthFix(action)
                    return alert ? alertActionLabel(alert) : null
                  }}
                  onFix={(action) => {
                    const alert = healthFix(action)
                    if (alert) {
                      runAlertAction(alert)
                    }
                  }}
                  onQuarantine={quarantineAnimal}
                />
              }
            >
              <AnimalMoves
                animal={selectedAnimal}
                habitats={snapshot.habitats}
                speciesLabels={(habitat) => habitatSpeciesLabel(habitat, snapshot.species_catalog)}
                target={transferTarget}
                preview={transferPreview}
                onTarget={setTransferTarget}
                onTransfer={transferAnimal}
                onRehome={rehomeAnimal}
              />
            </AnimalInspector>
          )}
          {!selectedDepot && !selectedAnimal && selectedGuest && (
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
                    <dd>
                      {selectedGuest.comfort.facility_id !== null
                        ? `Facility #${selectedGuest.comfort.facility_id}`
                        : `Habitat #${selectedGuest.target_habitat}`}
                    </dd>
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
                <NeedBar label="Toilet" value={selectedGuest.comfort.bladder} badWhenHigh />
                <NeedBar label="Value" value={selectedGuest.value_perception} />
                <GuestVisit guest={selectedGuest} />
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
                  {habitatSpeciesLabel(selectedHabitat, snapshot.species_catalog)}
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

                {selectedHabitat.animals > 0 && (
                  <>
                    <h3>Residents</h3>
                    <ResidentGroups habitat={selectedHabitat} />
                    <ul className="animal-roster" aria-label="Residents">
                      {snapshot.animals
                        .filter((animal) => animal.habitat_id === selectedHabitat.id)
                        .map((animal) => (
                          <li key={animal.id}>
                            <button onClick={() => selectAnimal(animal.id)}>
                              <span>{animal.name}</span>
                              <small>
                                #{animal.id} ·{" "}
                                {selectedHabitat.residents.length > 1 &&
                                  `${speciesLabel(animal.species, snapshot.species_catalog)} · `}
                                {animal.activity}
                              </small>
                            </button>
                          </li>
                        ))}
                    </ul>
                  </>
                )}

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
                    <>
                      <button className="shop-row" onClick={startGateMove}>
                        <span>
                          <b>Move gate</b>
                          <small>Then tap a fence segment; the tile outside stays clear</small>
                        </span>
                      </button>
                      <button
                        className={`shop-row${tool === "reshape" ? " active" : ""}`}
                        onClick={() => {
                          setTool(tool === "reshape" ? "select" : "reshape")
                          if (tool !== "reshape") {
                            inform(
                              "Draw the new fence along grid corners; the preview shows added and removed tiles.",
                            )
                          }
                        }}
                      >
                        <span>
                          <b>{tool === "reshape" ? "Stop reshaping" : "Reshape fence"}</b>
                          <small>Expand or shrink; animals, care and staff stay</small>
                        </span>
                      </button>
                    </>
                  )}
                </div>

                <h3>Fence</h3>
                <dl>
                  <div>
                    <dt>Type</dt>
                    <dd>
                      {selectedHabitat.fence.label} · {selectedHabitat.fence.height_cm} cm
                    </dd>
                  </div>
                </dl>
                {fenceOptions.map(({ fence, preview }) => (
                  <button
                    className="shop-row"
                    key={fence.key}
                    disabled={!preview.ok}
                    aria-label={`Rebuild fence as ${fence.label}`}
                    onClick={() => upgradeFence(fence.key)}
                  >
                    <span>
                      <b>
                        Rebuild as {fence.label} · {fence.height_cm} cm
                      </b>
                      <small>
                        {preview.ok
                          ? "Every segment at full price; nothing refunded · undoable"
                          : preview.message}
                      </small>
                    </span>
                    {preview.ok && <strong>{money(preview.cost_cents)}</strong>}
                  </button>
                ))}

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
                <HabitatDesignPanel
                  habitat={selectedHabitat}
                  catalog={snapshot.habitat_environment}
                  brush={brush}
                  designing={tool === "terrain"}
                  preview={environmentPreview}
                  onBrush={(next) => {
                    setBrush(next)
                    setTool("terrain")
                  }}
                  onToggleDesign={() =>
                    setTool((current) => (current === "terrain" ? "select" : "terrain"))
                  }
                  onRemoveObject={(objectId) => {
                    const game = gameRef.current
                    if (game) {
                      perform(() => game.remove_habitat_object(selectedHabitat.id, objectId))
                    }
                  }}
                />
                <HabitatUtilitiesPanel
                  habitat={selectedHabitat}
                  plants={snapshot.concessions}
                  onPriority={(priority) => {
                    const game = gameRef.current
                    if (game) {
                      perform(() => game.set_habitat_utility_priority(selectedHabitat.id, priority))
                    }
                  }}
                  onReplace={(plantId) => {
                    const game = gameRef.current
                    if (game) {
                      perform(() => game.replace_utility(plantId))
                    }
                  }}
                  onOpenPlant={selectConcession}
                />
                <button
                  className="shop-row"
                  data-testid="quarantine-toggle"
                  onClick={() => {
                    const game = gameRef.current
                    if (game) {
                      perform(() =>
                        game.set_habitat_quarantine(
                          selectedHabitat.id,
                          !selectedHabitat.quarantine,
                        ),
                      )
                    }
                  }}
                >
                  <span>
                    <b>
                      {selectedHabitat.quarantine
                        ? "Return to ordinary use"
                        : "Set aside for quarantine"}
                    </b>
                    <small>
                      {selectedHabitat.quarantine
                        ? "Quarantine: unwell animals rest apart here and go home once recovered"
                        : "An empty habitat can hold unwell animals apart from their group"}
                    </small>
                  </span>
                </button>
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
                <AdoptionOffers
                  offers={snapshot.species_catalog}
                  previews={adoptionPreviews}
                  count={adoptCount}
                  onCount={setAdoptCount}
                  onAdopt={adopt}
                  glyph={speciesGlyph}
                />
              </div>
            </>
          )}
          {!selectedDepot &&
            !selectedGuest &&
            !selectedHabitat &&
            !selectedZone &&
            selectedConcession && (
              <>
                <div className="window-title">
                  <span>
                    {snapshot.facility_catalog.find(
                      (offer) => offer.key === selectedConcession.kind,
                    )?.label ?? selectedConcession.kind}{" "}
                    #{selectedConcession.id}
                  </span>
                  <button
                    onClick={() => setSelectedConcessionId(null)}
                    aria-label="Close stand inspector"
                  >
                    ×
                  </button>
                </div>
                <div className="manager-card stand-card">
                  <dl>
                    <div
                      className={
                        selectedConcession.service_state === "healthy" ? "" : "zone-warning"
                      }
                    >
                      <dt>Condition</dt>
                      <dd>
                        {selectedConcession.condition}% · {selectedConcession.service_state}
                      </dd>
                    </div>
                    <div>
                      <dt>Maintenance</dt>
                      <dd>{selectedConcession.maintenance_status}</dd>
                    </div>
                    {!selectedConcession.utility && (
                      <div
                        className={
                          selectedConcession.customer_tiles.length > 0 ? "" : "zone-warning"
                        }
                      >
                        <dt>Customers</dt>
                        <dd data-stand-customers>
                          {selectedConcession.customer_tiles.length > 0
                            ? `Public side ${selectedConcession.customer_tiles
                                .map((tile) => `${tile.x},${tile.y}`)
                                .join(" · ")}`
                            : "No public path: guests cannot buy here"}
                        </dd>
                      </div>
                    )}
                    <div className={selectedConcession.active_service_point ? "" : "zone-warning"}>
                      <dt>Service entrance</dt>
                      <dd data-stand-service>
                        {selectedConcession.active_service_point
                          ? (() => {
                              const used = selectedConcession.active_service_point
                              const point = selectedConcession.service_points.find(
                                (candidate) => candidate.x === used.x && candidate.y === used.y,
                              )
                              const others = selectedConcession.service_points.length - 1
                              return `${used.x},${used.y} via ${point?.access === "staff" ? "staff-only path" : "public path"}${others > 0 ? ` · ${others} alternative${others === 1 ? "" : "s"}` : ""}`
                            })()
                          : "No service point reachable from the depot"}
                      </dd>
                    </div>
                    {selectedConcession.utility && (
                      <UtilityPlantRows utility={selectedConcession.utility} />
                    )}
                    {!selectedConcession.utility && selectedConcession.comfort && (
                      <ComfortRows
                        comfort={selectedConcession.comfort}
                        demand={snapshot.operations.comfort}
                      />
                    )}
                    {!selectedConcession.utility && !selectedConcession.comfort && (
                      <>
                        <div>
                          <dt>Price</dt>
                          <dd>{money(selectedConcession.price_cents)}</dd>
                        </div>
                        {selectedConcession.stand && (
                          <StandRows
                            stand={selectedConcession.stand}
                            commands={standCommands(selectedConcession.id)}
                          />
                        )}
                        <div>
                          <dt>Revenue</dt>
                          <dd>{money(selectedConcession.total_revenue_cents)} all time</dd>
                        </div>
                      </>
                    )}
                  </dl>
                  {selectedConcession.utility && (
                    <UtilityPlantActions
                      utility={selectedConcession.utility}
                      onReplace={() => {
                        const game = gameRef.current
                        if (game) {
                          perform(() => game.replace_utility(selectedConcession.id))
                        }
                      }}
                    />
                  )}
                  {selectedConcession.stand && (
                    <StandActions
                      stand={selectedConcession.stand}
                      commands={standCommands(selectedConcession.id)}
                    />
                  )}
                  {selectedConcession.comfort?.janitor_status &&
                    snapshot.animal_care_depot.janitors.length === 0 && (
                      <button className="shop-row" onClick={hireJanitor}>
                        <span>
                          <b>Hire janitor</b>
                          <small>Janitors clean toilets and empty bins as well as paths</small>
                        </span>
                        <strong>{money(snapshot.animal_care_depot.janitor_hire_cost_cents)}</strong>
                      </button>
                    )}
                  {(selectedConcession.comfort === null ||
                    selectedConcession.comfort.service === "toilet") &&
                    snapshot.animal_care_depot.mechanics.length === 0 && (
                      <button className="shop-row" onClick={hireMechanic}>
                        <span>
                          <b>Hire mechanic</b>
                          <small>Mechanics repair worn stands before they break down</small>
                        </span>
                        <strong>
                          {money(snapshot.animal_care_depot.mechanic_hire_cost_cents)}
                        </strong>
                      </button>
                    )}
                </div>
              </>
            )}
          {!selectedDepot &&
            !selectedGuest &&
            !selectedHabitat &&
            !selectedZone &&
            !selectedAnimal &&
            !selectedConcession && (
              <>
                <div className="window-title">
                  <span>Park manager</span>
                </div>
                <div className="manager-card">
                  <h2>Alerts</h2>
                  <AlertList
                    alerts={snapshot.alerts}
                    limit={3}
                    onJump={jumpTo}
                    onAction={runAlertAction}
                    actionLabel={alertActionLabel}
                  />
                  <h3>Today</h3>
                  <DailySummaryView snapshot={snapshot} onJump={jumpTo} />
                  <button className="secondary" onClick={() => setWorkspaceTab("summary")}>
                    Open management workspace
                  </button>
                  {(snapshot.tutorial.status === "skipped" ||
                    snapshot.tutorial.status === "completed") && (
                    <button className="secondary" onClick={restartTutorial}>
                      Restart guide
                    </button>
                  )}
                  <button
                    className="secondary"
                    onClick={() => {
                      setMenuView("new")
                      setMenuOpen(true)
                    }}
                  >
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
          {/* One row: the shape toggle, then the outline status or the rectangle size. */}
          <div className="habitat-tools">
            {tool === "path" && (
              <div className="path-brush" role="group" aria-label="Path brush">
                {(["public", "staff"] as const).map((access) => (
                  <button
                    key={access}
                    data-path-access={access}
                    className={pathBrush.access === access ? "active" : ""}
                    aria-pressed={pathBrush.access === access}
                    onClick={() => setPathBrush((current) => ({ ...current, access }))}
                  >
                    {access === "public" ? "Public" : "Staff only"}
                  </button>
                ))}
                {([1, 2, 3] as const).map((width) => (
                  <button
                    key={width}
                    data-path-width={width}
                    className={
                      pathBrush.shape === "stroke" && pathBrush.width === width ? "active" : ""
                    }
                    aria-pressed={pathBrush.shape === "stroke" && pathBrush.width === width}
                    aria-label={`Path width ${width}`}
                    onClick={() =>
                      setPathBrush((current) => ({ ...current, shape: "stroke", width }))
                    }
                  >
                    {width}×
                  </button>
                ))}
                <button
                  data-path-shape="plaza"
                  className={pathBrush.shape === "plaza" ? "active" : ""}
                  aria-pressed={pathBrush.shape === "plaza"}
                  onClick={() => setPathBrush((current) => ({ ...current, shape: "plaza" }))}
                >
                  ▭ Plaza
                </button>
                <PlanToggle planning={planning} onToggle={() => setPlanning((on) => !on)} />
                {constructionPreview && (
                  <span
                    className={`path-preview ${constructionPreview.ok ? "valid" : "invalid"}`}
                    role="status"
                  >
                    {constructionPreview.message}
                  </span>
                )}
              </div>
            )}
            {tool === "habitat" && (
              <div className="habitat-shape" role="group" aria-label="Fence shape">
                {(["rect", "outline"] as const).map((shape) => (
                  <button
                    key={shape}
                    className={habitatShape === shape ? "active" : ""}
                    aria-pressed={habitatShape === shape}
                    onClick={() => setHabitatShape(shape)}
                  >
                    {shape === "rect" ? "▭ Rectangle" : "⌐ Outline"}
                  </button>
                ))}
                <PlanToggle planning={planning} onToggle={() => setPlanning((on) => !on)} />
              </div>
            )}
            {outlineMode && (
              <div
                className={`outline-status ${placement?.ok ? "valid" : "invalid"}`}
                role="status"
                aria-label="Fence outline"
                data-corners={outline.map((corner) => `${corner.x},${corner.y}`).join(" ")}
              >
                <span>
                  {placement === null ? (
                    "Tap or drag along grid corners to start the fence."
                  ) : (
                    <>
                      <b>
                        {placement.ok ? "✓" : "!"}{" "}
                        {placement.area > 0
                          ? `${placement.area} tiles · fence ${placement.perimeter} · ${money(placement.cost_cents)}`
                          : `${outline.length} corners`}
                      </b>{" "}
                      {placement.message}
                      {placement.warnings.map((warning) => ` · ${warning}`).join("")}
                    </>
                  )}
                </span>
                {outline.length > 0 && <button onClick={cancelOutline}>Clear</button>}
              </div>
            )}
            {tool === "habitat" && habitatShape === "rect" && (
              <div className="habitat-size" aria-label="Habitat size">
                {(["width", "height"] as const).map((side) => (
                  <label key={side}>
                    {side === "width" ? "W" : "H"}
                    <input
                      type="number"
                      min={1}
                      max={20}
                      value={habitatSize[side]}
                      aria-label={`Habitat ${side}`}
                      onChange={(event) => {
                        const value = Math.round(Number(event.target.value))
                        if (Number.isFinite(value) && value >= 1 && value <= 20) {
                          setHabitatSize((size) => ({ ...size, [side]: value }))
                        }
                      }}
                    />
                  </label>
                ))}
                <button
                  onClick={() =>
                    setHabitatSize((size) => ({ width: size.height, height: size.width }))
                  }
                  aria-label="Rotate habitat"
                >
                  ⟳ Rotate
                </button>
              </div>
            )}
          </div>
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
            label="Path"
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
            .filter((offer) => offer.service in FACILITY_ICONS)
            .map((offer) => (
              <ToolButton
                key={offer.key}
                active={tool === `facility:${offer.key}`}
                icon={FACILITY_ICONS[offer.service] ?? "▥"}
                label={`${offer.label} · ${money(offer.build_cost_cents)}`}
                onClick={() => setTool(`facility:${offer.key}`)}
              />
            ))}
          {snapshot.land.parcels.length > 0 && (
            <ToolButton
              active={tool === "land"}
              icon="⛳"
              label="Land"
              onClick={() => setTool("land")}
            />
          )}
          <ToolButton
            active={tool === "bulldoze"}
            icon="⌫"
            label="Demolish"
            onClick={() => setTool("bulldoze")}
          />
          <HistoryButton
            icon="↶"
            label="Undo"
            shortcut="Ctrl+Z"
            step={snapshot.construction.undo}
            onClick={undoConstruction}
          />
          <HistoryButton
            icon="↷"
            label="Redo"
            shortcut="Ctrl+Shift+Z"
            step={snapshot.construction.redo}
            onClick={redoConstruction}
          />
        </nav>
      </footer>
    </main>
  )
}

/** Plan mode (#99): finished path and habitat gestures join the plan instead of building. */
function PlanToggle({ planning, onToggle }: { planning: boolean; onToggle: () => void }) {
  return (
    <button
      data-plan-toggle
      className={planning ? "active" : ""}
      aria-pressed={planning}
      title="Plan without spending: gestures are added to the expansion plan"
      onClick={onToggle}
    >
      ✎ Plan
    </button>
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

/** Undo or redo of the latest construction step; Rust says whether it can run and why not. */
function HistoryButton({
  icon,
  label,
  shortcut,
  step,
  onClick,
}: {
  icon: string
  label: string
  shortcut: string
  step: ConstructionStep | null
  onClick: () => void
}) {
  const title = step
    ? `${label} ${step.label}${step.cost_cents > 0 ? ` (${money(step.cost_cents)})` : ""} · ${shortcut}${
        step.ok ? "" : ` · ${step.message}`
      }`
    : `Nothing to ${label.toLowerCase()} · ${shortcut}`
  return (
    <button
      className={`tool history ${step && !step.ok ? "blocked" : ""}`}
      onClick={onClick}
      disabled={!step}
      title={title}
      aria-label={label}
    >
      <span>{icon}</span>
      <small>{label}</small>
    </button>
  )
}

/** Inspector rows for a toilet, bench or bin: Rust's occupancy, upkeep and service state (#101). */
function ComfortRows({ comfort, demand }: { comfort: ComfortFacility; demand: ComfortOperations }) {
  let unmet: number | null = null
  if (comfort.service === "toilet") {
    unmet = demand.toilet_demand_unmet
  } else if (comfort.service === "bench") {
    unmet = demand.rest_demand_unmet
  }
  return (
    <>
      <div className={comfort.open ? "" : "zone-warning"}>
        <dt>{comfort.service === "bin" ? "Fill" : "Occupancy"}</dt>
        <dd data-comfort-status>{comfort.status}</dd>
      </div>
      {comfort.cleanliness !== null && (
        <div className={comfort.cleanliness > 55 ? "" : "zone-warning"}>
          <dt>Cleanliness</dt>
          <dd>{comfort.cleanliness}%</dd>
        </div>
      )}
      {comfort.janitor_status !== null && (
        <div>
          <dt>Janitor</dt>
          <dd data-comfort-janitor>{comfort.janitor_status}</dd>
        </div>
      )}
      <div>
        <dt>{comfort.service === "bin" ? "Wrappers taken" : "Guests served"}</dt>
        <dd data-comfort-uses>{comfort.total_uses}</dd>
      </div>
      {unmet !== null && (
        <div className={unmet > 0 ? "zone-warning" : ""}>
          <dt>Unmet demand</dt>
          <dd data-comfort-unmet>
            {unmet === 0
              ? "None"
              : `${unmet} guest${unmet === 1 ? "" : "s"} found no ${comfort.service === "toilet" ? "toilet" : "seat"}`}
          </dd>
        </div>
      )}
    </>
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

/**
 * The scenario's state-backed goals (#91). In a guided scenario (#93) it is the guide: the
 * next step shows its hint, the park's own blocker and a pointer to the tool it needs. Every
 * tick comes from Rust park state.
 */
function ObjectiveChecklist({
  scenario,
  tutorial,
  open,
  onToggle,
  focusLabel,
  onFocus,
  onShow,
  onSkip,
}: {
  scenario: ScenarioState
  tutorial: Tutorial
  open: boolean
  onToggle: (open: boolean) => void
  focusLabel: string | null
  onFocus: () => void
  onShow: (focus: Point) => void
  onSkip: () => void
}) {
  const guided = tutorial.status === "active"
  const next = guided
    ? scenario.objectives.find((objective) => objective.id === tutorial.next)
    : scenario.objectives.find((objective) => !objective.done)
  const doneCount = scenario.objectives.filter((objective) => objective.done).length
  // A step whose structure was undone is next again; show it open, not ticked.
  const ticked = (objective: Objective) => objective.done && objective !== next
  return (
    <details
      className="tutorial-card bevel"
      open={open}
      onToggle={(event) => onToggle(event.currentTarget.open)}
      aria-label={guided ? "Park guide" : "Scenario objectives"}
    >
      <summary>
        {scenario.title} · {doneCount}/{scenario.objectives.length}
        {!open && next ? ` · ${next.title}` : ""}
      </summary>
      <ol>
        {scenario.objectives.map((objective) => (
          <li
            key={objective.id}
            data-objective={objective.id}
            className={[ticked(objective) && "done", objective === next && "next"]
              .filter(Boolean)
              .join(" ")}
            aria-current={objective === next ? "step" : undefined}
          >
            <span aria-hidden="true">{ticked(objective) ? "✓" : "○"}</span> {objective.title}
          </li>
        ))}
      </ol>
      {next?.hint != null && <p className="tutorial-hint">{next.hint}</p>}
      {guided && tutorial.problem !== null && (
        <p className="tutorial-problem" role="status">
          {tutorial.problem}
        </p>
      )}
      <div className="tutorial-actions">
        {guided && focusLabel !== null && <button onClick={onFocus}>{focusLabel}</button>}
        {next && <button onClick={() => onShow(next.focus)}>Show next goal</button>}
        {guided && <button onClick={onSkip}>Skip guide</button>}
      </div>
    </details>
  )
}

/**
 * The compact watch label of a followed animal (#141): zoo-core's name and current activity, a
 * deliberate route to the inspector and an explicit way to stop following (also Escape or a pan).
 */
function AnimalWatch({
  animal,
  speciesLabel,
  detailsOpen,
  onDetails,
  onStop,
}: {
  animal: Animal
  speciesLabel: string
  detailsOpen: boolean
  onDetails: () => void
  onStop: () => void
}) {
  const activity = animal.activity === "walking" ? "Walking" : "Resting"
  return (
    <div
      className="animal-watch bevel"
      role="group"
      aria-label={`Watching ${animal.name}`}
      data-animal-watch={animal.id}
    >
      <span className="animal-watch-text">
        <b>{animal.name}</b>
        <small>
          <span data-testid="watch-activity">{activity}</span> · {speciesLabel}
        </small>
      </span>
      {!detailsOpen && (
        <button onClick={onDetails} data-action="animal-details">
          Details
        </button>
      )}
      <button onClick={onStop} data-action="release-follow" aria-label="Stop following">
        ×
      </button>
    </div>
  )
}

/**
 * One animal's identity, authoritative activity and individual needs (#96), beside the habitat
 * resources and welfare components it shares with its group. Every value comes from zoo-core.
 */
function AnimalInspector({
  animal,
  habitat,
  speciesLabel,
  name,
  onNameChange,
  onRename,
  onLocate,
  onShowHabitat,
  onClose,
  health,
  children,
}: {
  animal: Animal
  habitat: Habitat | null
  speciesLabel: string
  name: string
  onNameChange: (name: string) => void
  onRename: () => void
  onLocate: () => void
  onShowHabitat: () => void
  onClose: () => void
  /** Its health, treatment and quarantine (#109), shown first. */
  health: ReactNode
  /** Moving or rehoming the animal (#108). */
  children: ReactNode
}) {
  let activity = "Resting"
  if (animal.activity === "walking") {
    activity = animal.target ? `Walking to (${animal.target.x}, ${animal.target.y})` : "Walking"
  }
  // The animal's own species group in its habitat (#108).
  const group = habitat?.residents.find((resident) => resident.species === animal.species)
  return (
    <>
      <div className="window-title">
        <span>
          {animal.name} · #{animal.id}
        </span>
        <button onClick={onClose} aria-label="Close animal inspector">
          ×
        </button>
      </div>
      <div className="animal-card" data-animal-id={animal.id}>
        <div className="habitat-name">{speciesLabel}</div>
        <div className="guest-thought" data-testid="animal-activity">
          {activity}
        </div>
        {health}
        <form
          className="animal-rename"
          onSubmit={(event) => {
            event.preventDefault()
            onRename()
          }}
        >
          <label>
            <span>Name</span>
            <input
              value={name}
              maxLength={24}
              onChange={(event) => onNameChange(event.target.value)}
              aria-label="Animal name"
            />
          </label>
          <button type="submit" disabled={name.trim() === "" || name.trim() === animal.name}>
            Rename
          </button>
        </form>
        <dl>
          <div>
            <dt>Sex</dt>
            <dd>{animal.sex}</dd>
          </div>
          <div>
            <dt>Life stage</dt>
            <dd>{animal.life_stage}</dd>
          </div>
          <div>
            <dt>Arrived</dt>
            <dd>Day {animal.arrived_day}</dd>
          </div>
          <div>
            <dt>Position</dt>
            <dd>
              ({animal.x}, {animal.y}) · habitat #{animal.habitat_id}
            </dd>
          </div>
        </dl>
        <h3>Own needs</h3>
        <NeedBar label="Energy" value={animal.energy} />
        {habitat && (
          <>
            <h3>Shared with habitat #{habitat.id}</h3>
            <NeedBar label="Food" value={habitat.food} />
            <NeedBar label="Water" value={habitat.water} />
            <NeedBar label="Cleanliness" value={habitat.cleanliness} />
            <NeedBar label="Social fit" value={group?.social_score ?? habitat.social_score} />
            <NeedBar label="Space fit" value={habitat.space_score} />
            <dl>
              <div>
                <dt>Shelter</dt>
                <dd>
                  {habitat.environment.assessment.shelter_capacity > 0
                    ? `For ${habitat.environment.assessment.shelter_capacity}`
                    : "None reachable"}
                </dd>
              </div>
              <div>
                <dt>Group welfare</dt>
                <dd>
                  {animal.welfare}% → {group?.welfare_target ?? habitat.welfare_target}%
                </dd>
              </div>
            </dl>
            <div className="guest-thought">{habitat.welfare_status}</div>
          </>
        )}
        {children}
        <div className="animal-actions">
          <button onClick={onLocate}>Locate</button>
          <button onClick={onShowHabitat}>Show habitat</button>
        </div>
      </div>
    </>
  )
}
