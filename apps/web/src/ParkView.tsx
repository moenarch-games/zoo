import {
  createThreeSceneRenderer,
  createWorldProjector,
  type RendererCamera,
  type RendererInstanceBatch,
  type RendererWorkObservations,
  type ThreeSceneRenderer,
} from "@moritzbrantner/three-d-renderer"
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react"

import { loadEffectsKit } from "./effects-kit"
import type { Point, Snapshot } from "./game-types"
import { ParkEffects, type ParkEffectsState } from "./park-effects"
import {
  buildActorFrame,
  buildSceneryNodes,
  buildStaticNodes,
  effectSources,
  sceneryKey,
  type EffectSources,
  type PickAnchor,
  type SceneOverlay,
  type Vec3,
} from "./park-scene"
import { batchNodes } from "./scene-batching"
import initScene, { ParkCameraBridge } from "./scene-wasm/zoo_scene"
import { loadSceneryKit, type SceneryKit } from "./scenery-kit"

// Browser adapter for the Zoo park view. It owns pointer/touch/keyboard gesture interpretation
// and the render loop. Camera policy and picking come from zoo-scene (over 3d-lab camera and
// projective math); rendering and world→screen projection come from the shared 3d-lab renderer.

export type ParkPick =
  | { kind: "guest"; id: number; tile: Point | null }
  | { kind: "animal"; habitatId: number; tile: Point | null }
  | { kind: "concession"; id: number; tile: Point }
  | { kind: "depot"; tile: Point | null }
  | { kind: "tile"; tile: Point }
  | { kind: "none" }

type Props = {
  snapshot: Snapshot
  overlay: SceneOverlay
  paused: boolean
  /** When true, a primary drag starting on a park tile drives the tool instead of panning. */
  dragTool: boolean
  /** Each new token reframes the camera: on `focus` (a scenario objective) or the whole park. */
  camera: CameraRequest
  tooltip?: ReactNode
  onTileDown: (tile: Point) => void
  onTileDrag: (tile: Point) => void
  onToolGestureEnd: (commit: boolean) => void
  onHover: (tile: Point | null) => void
  onPick: (pick: ParkPick) => void
  /** Reported once when the renderer fails (startup, a render exception or WebGL context loss). */
  onRendererFailure?: (message: string) => void
}

export type CameraRequest = { token: number; focus: Point | null }

/** Closer than the whole-park frame, so an objective reads clearly; the player zooms freely. */
const FOCUS_ZOOM = 1.8

type CameraFrame = RendererCamera & {
  eye: Vec3
  target: Vec3
  yawDegrees: number
  pitchDegrees: number
  zoom: number
}

type BridgePick = { ground: [number, number]; tile: Point | null } | null

type Gesture =
  | { kind: "pending"; pointerId: number; startX: number; startY: number }
  | { kind: "pan"; pointerId: number }
  | { kind: "orbit"; pointerId: number }
  | { kind: "tool"; pointerId: number; tileKey: string }
  /**
   * A touch that landed on a park tile with a drag tool selected. Nothing is sent to the
   * tool until the touch is known to be single-finger (it moves past the slop or lifts),
   * so the first finger of a pinch, twist or tilt never places anything.
   */
  | { kind: "toolPending"; pointerId: number; tile: Point; startX: number; startY: number }
  | {
      kind: "multi"
      mode: "undecided" | "transform" | "tilt"
      start: TwoPointerState
      mid: [number, number]
      distance: number
      angle: number
    }

type TwoPointerState = { mid: [number, number]; distance: number; angle: number }

const BACKGROUND = "#a9d6e5"
const CLICK_SLOP_MOUSE = 5
const CLICK_SLOP_TOUCH = 10
/** Wheel zoom per delta unit, by WheelEvent.deltaMode (pixel, line, page). */
const WHEEL_SCALE_BY_DELTA_MODE: Record<number, number> = { 0: 0.0015, 1: 0.05, 2: 1 }
const ORBIT_DEGREES_PER_PIXEL = 0.35
const TILT_DEGREES_PER_PIXEL = 0.25
const ACTOR_SMOOTHING_PER_SECOND = 7
/** Two-finger movement (px) before deciding between tilt and pan/zoom/rotate. */
const MULTI_TOUCH_DECISION_PX = 14

type ViewDebugHook = {
  ready: boolean
  camera: () => { yawDegrees: number; pitchDegrees: number; zoom: number; target: Vec3 } | null
  projectWorld: (point: Vec3) => { x: number; y: number; visible: boolean } | null
  parkFootprintInFrame: () => boolean
  /** Scene parts submitted, counting each instance. */
  nodeCount: () => number
  /** Renderer objects submitted: individual nodes plus one per instance batch. */
  drawCount: () => number
  /** Live shared-renderer caches and the GPU resources Three.js holds for this mount. */
  resources: () => RendererResources | null
  /** Cosmetic particle bursts (#86): live/started per effect and the renderer's atlas count. */
  effects: () => (ParkEffectsState & { reducedMotion: boolean; liveAtlases: number }) | null
}

type RendererResources = {
  objects: number
  geometries: number
  materials: number
  gpuGeometries: number
  gpuTextures: number
  gpuPrograms: number
}

/** The slice of Three.js' WebGLRenderer that the park view reads for resource accounting. */
type GpuRenderer = {
  info: {
    memory: { geometries: number; textures: number }
    programs: readonly unknown[] | null
  }
  getContext(): { isContextLost(): boolean }
  forceContextLoss(): void
}

/** Renderer mounts and releases across park-view remounts, and what the last release left. */
type RendererLifetime = {
  mounts: number
  releases: number
  lastRelease: { gpuGeometries: number; gpuTextures: number } | null
}

const lifetime: RendererLifetime = { mounts: 0, releases: 0, lastRelease: null }

declare global {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- global augmentation requires interface merging.
  interface Window {
    __zooParkView?: ViewDebugHook
    __zooRendererLifetime?: Readonly<RendererLifetime>
  }
}

export default function ParkView(props: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const tooltipRef = useRef<HTMLDivElement | null>(null)
  const propsRef = useRef(props)
  propsRef.current = props
  const sceneDirtyRef = useRef(true)
  const bridgeRef = useRef<ParkCameraBridge | null>(null)
  const cameraDirtyRef = useRef(true)
  const appliedCameraRef = useRef(-1)
  const applyCameraRequest = useCallback(() => {
    const bridge = bridgeRef.current
    const request = propsRef.current.camera
    if (!bridge || request.token === appliedCameraRef.current) {
      return
    }
    appliedCameraRef.current = request.token
    if (request.focus) {
      bridge.focus_tile(request.focus.x, request.focus.y, FOCUS_ZOOM)
    } else {
      bridge.reset()
    }
    cameraDirtyRef.current = true
  }, [])
  const [status, setStatus] = useState<"loading" | "ready" | "failed">("loading")

  // Any new snapshot or overlay requires a full scene rebuild on the next frame.
  useEffect(() => {
    sceneDirtyRef.current = true
  }, [props.snapshot, props.overlay])

  const { width: parkWidth, height: parkHeight } = props.snapshot

  useEffect(() => {
    const container = containerRef.current
    const canvas = canvasRef.current
    if (!container || !canvas) {
      return
    }

    let disposed = false
    let renderer: ThreeSceneRenderer | null = null
    let gpu: GpuRenderer | null = null
    let work: RendererWorkObservations | null = null
    let bridge: ParkCameraBridge | null = null
    let kit: SceneryKit | null = null
    let effects: ParkEffects | null = null
    let sources: EffectSources = { foliage: [], ponds: [] }
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)")
    let camera: CameraFrame | null = null
    let frameHandle = 0
    let viewport = { width: 1, height: 1 }
    let anchors: PickAnchor[] = []
    let nodeCount = 0
    let drawCount = 0
    let staticKey: { snapshot: Snapshot; overlay: SceneOverlay } | null = null
    let staticGeneration = 0
    let staticBatches: RendererInstanceBatch[] = []
    let scenery: { key: string; batches: RendererInstanceBatch[] } | null = null
    let lastTime = performance.now()
    let hoverClient: [number, number] | null = null
    let hoverKey = ""
    let gesture: Gesture | null = null
    const pointers = new Map<number, { x: number; y: number; type: string }>()
    const actors = new Map<string, { x: number; z: number; seen: boolean }>()
    let actorsMoving = false

    const aspect = () => viewport.width / viewport.height

    const toNdc = (clientX: number, clientY: number): [number, number] => {
      const rect = canvas.getBoundingClientRect()
      return [
        ((clientX - rect.left) / rect.width) * 2 - 1,
        1 - ((clientY - rect.top) / rect.height) * 2,
      ]
    }

    const pick = (clientX: number, clientY: number): BridgePick => {
      if (!bridge) {
        return null
      }
      const [x, y] = toNdc(clientX, clientY)
      return JSON.parse(bridge.pick_json(x, y, aspect())) as BridgePick
    }

    const pickTile = (clientX: number, clientY: number) => pick(clientX, clientY)?.tile ?? null

    const pickAnchor = (clientX: number, clientY: number): PickAnchor | null => {
      if (!camera) {
        return null
      }
      const rect = canvas.getBoundingClientRect()
      const px = clientX - rect.left
      const py = clientY - rect.top
      const project = createWorldProjector(camera, viewport)
      let best: { anchor: PickAnchor; score: number } | null = null
      for (const anchor of anchors) {
        const center = project(anchor.point)
        if (!center.visible) {
          continue
        }
        const edge = project([anchor.point[0], anchor.point[1] + anchor.radius, anchor.point[2]])
        const radius = Math.max(12, Math.hypot(edge.x - center.x, edge.y - center.y))
        const score = Math.hypot(px - center.x, py - center.y) / radius
        if (score <= 1 && (!best || score < best.score)) {
          best = { anchor, score }
        }
      }
      return best?.anchor ?? null
    }

    const resolvePick = (clientX: number, clientY: number): ParkPick => {
      const tile = pickTile(clientX, clientY)
      const anchor = pickAnchor(clientX, clientY)
      if (anchor) {
        switch (anchor.kind) {
          case "guest":
            return { kind: "guest", id: anchor.id, tile }
          case "animal":
            return { kind: "animal", habitatId: anchor.habitatId, tile }
          case "concession":
            return { kind: "concession", id: anchor.id, tile: anchor.tile }
          case "depot":
            return { kind: "depot", tile }
        }
      }
      return tile ? { kind: "tile", tile } : { kind: "none" }
    }

    const resolveActor = (key: string, x: number, z: number, dt: number): [number, number] => {
      const current = actors.get(key)
      if (!current || Math.hypot(current.x - x, current.z - z) > 3) {
        actors.set(key, { x, z, seen: true })
        return [x, z]
      }
      const blend = 1 - Math.exp(-dt * ACTOR_SMOOTHING_PER_SECOND)
      current.x += (x - current.x) * blend
      current.z += (z - current.z) * blend
      current.seen = true
      if (Math.hypot(current.x - x, current.z - z) > 0.005) {
        actorsMoving = true
      }
      return [current.x, current.z]
    }

    // Disposes the shared renderer and releases this mount's WebGL context rather than waiting for
    // the detached canvas to be collected, so repeated mounts never accumulate live contexts.
    const release = () => {
      if (!renderer) {
        return
      }
      renderer.dispose()
      renderer = null
      work = null
      if (gpu) {
        const { memory } = gpu.info
        lifetime.lastRelease = { gpuGeometries: memory.geometries, gpuTextures: memory.textures }
        if (!gpu.getContext().isContextLost()) {
          gpu.forceContextLoss()
        }
        gpu = null
      }
      lifetime.releases += 1
    }

    let failed = false
    const failClosed = (error: unknown) => {
      if (failed) {
        return
      }
      failed = true
      console.error("Shared 3d-lab park renderer failed; failing closed", error)
      propsRef.current.onRendererFailure?.(error instanceof Error ? error.message : String(error))
      release()
      if (!disposed) {
        setStatus("failed")
      }
    }

    const renderFrame = (now: number) => {
      frameHandle = window.requestAnimationFrame(renderFrame)
      if (!renderer || !bridge || !kit) {
        return
      }
      const dt = Math.min((now - lastTime) / 1000, 0.1)
      lastTime = now
      const current = propsRef.current
      try {
        let cameraChanged = false
        if (cameraDirtyRef.current || !camera) {
          camera = JSON.parse(bridge.frame_json(aspect())) as CameraFrame
          cameraDirtyRef.current = false
          cameraChanged = true
        }
        const animate = !current.paused
        if (sceneDirtyRef.current || animate || actorsMoving || effects?.active) {
          sceneDirtyRef.current = false
          actorsMoving = false
          for (const actor of actors.values()) {
            actor.seen = false
          }
          if (staticKey?.snapshot !== current.snapshot || staticKey.overlay !== current.overlay) {
            staticKey = { snapshot: current.snapshot, overlay: current.overlay }
            staticGeneration += 1
            staticBatches = batchNodes(
              "static",
              buildStaticNodes(current.snapshot, current.overlay, kit),
              String(staticGeneration),
            )
            sources = effectSources(current.snapshot)
            const key = sceneryKey(current.snapshot)
            if (scenery?.key !== key) {
              scenery = {
                key,
                batches: batchNodes("scenery", buildSceneryNodes(current.snapshot, kit), key),
              }
            }
          }
          const frame = buildActorFrame({
            snapshot: current.snapshot,
            overlay: current.overlay,
            timeSeconds: now / 1000,
            animate,
            resolveActor: (key, x, z) => resolveActor(key, x, z, dt),
          })
          for (const [key, actor] of actors) {
            if (!actor.seen) {
              actors.delete(key)
            }
          }
          anchors = frame.anchors
          const instanceBatches = scenery ? scenery.batches.concat(staticBatches) : staticBatches
          nodeCount =
            frame.nodes.length +
            instanceBatches.reduce((sum, batch) => sum + batch.instances.length, 0)
          drawCount = frame.nodes.length + instanceBatches.length
          // Cosmetic clock in seconds; omitted effects (none loaded) hold no effect resources.
          const effectFrame = effects?.frame(
            now / 1000,
            frame.presence,
            sources,
            animate,
            !reducedMotion.matches,
          )
          work = renderer.render({
            camera,
            nodes: frame.nodes,
            instanceBatches,
            ...(effectFrame ? { effects: effectFrame } : {}),
          })
        } else if (cameraChanged) {
          work = renderer.renderCamera(camera)
        }

        if (hoverClient && gesture === null) {
          const tile = pickTile(hoverClient[0], hoverClient[1])
          const key = tile ? `${tile.x}:${tile.y}` : ""
          if (key !== hoverKey) {
            hoverKey = key
            current.onHover(tile)
          }
        }
      } catch (error) {
        failClosed(error)
      }
    }

    const resize = () => {
      const rect = container.getBoundingClientRect()
      viewport = { width: Math.max(1, rect.width), height: Math.max(1, rect.height) }
      renderer?.setSize(viewport.width, viewport.height, window.devicePixelRatio || 1)
      cameraDirtyRef.current = true
      sceneDirtyRef.current = true
    }
    const resizeObserver = new ResizeObserver(resize)

    // --- Gestures -------------------------------------------------------------------------

    const panBetween = (from: [number, number], to: [number, number]) => {
      bridge?.pan_drag(...toNdc(...from), ...toNdc(...to), aspect())
      cameraDirtyRef.current = true
    }

    const orbitBy = (dx: number, dy: number) => {
      bridge?.orbit_by_degrees(-dx * ORBIT_DEGREES_PER_PIXEL, dy * TILT_DEGREES_PER_PIXEL)
      cameraDirtyRef.current = true
    }

    const zoomAt = (factor: number, clientX: number, clientY: number) => {
      bridge?.zoom_at(factor, ...toNdc(clientX, clientY), aspect())
      cameraDirtyRef.current = true
    }

    const twoPointerState = (): TwoPointerState => {
      const [a, b] = [...pointers.values()]
      if (!a || !b) {
        throw new Error("twoPointerState requires two active pointers")
      }
      return {
        mid: [(a.x + b.x) / 2, (a.y + b.y) / 2] as [number, number],
        distance: Math.max(1, Math.hypot(b.x - a.x, b.y - a.y)),
        angle: Math.atan2(b.y - a.y, b.x - a.x),
      }
    }

    const endToolGesture = (commit: boolean) => {
      if (gesture?.kind === "tool") {
        propsRef.current.onToolGestureEnd(commit)
      }
    }

    const onPointerDown = (event: PointerEvent) => {
      if (!bridge) {
        return
      }
      canvas.focus({ preventScroll: true })
      canvas.setPointerCapture(event.pointerId)
      pointers.set(event.pointerId, { x: event.clientX, y: event.clientY, type: event.pointerType })
      event.preventDefault()

      if (pointers.size === 2) {
        endToolGesture(false)
        const state = twoPointerState()
        gesture = { kind: "multi", mode: "undecided", start: state, ...state }
        return
      }
      if (pointers.size > 2) {
        return
      }

      const mouse = event.pointerType === "mouse"
      if (mouse && (event.button === 2 || (event.button === 0 && event.ctrlKey))) {
        gesture = { kind: "orbit", pointerId: event.pointerId }
        return
      }
      if (mouse && event.button === 1) {
        gesture = { kind: "pan", pointerId: event.pointerId }
        return
      }
      if (event.button !== 0) {
        return
      }

      if (propsRef.current.dragTool) {
        const tile = pickTile(event.clientX, event.clientY)
        if (tile && !mouse) {
          gesture = {
            kind: "toolPending",
            pointerId: event.pointerId,
            tile,
            startX: event.clientX,
            startY: event.clientY,
          }
          return
        }
        if (tile) {
          gesture = { kind: "tool", pointerId: event.pointerId, tileKey: `${tile.x}:${tile.y}` }
          propsRef.current.onTileDown(tile)
          return
        }
      }
      gesture = {
        kind: "pending",
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
      }
    }

    const dragToolTo = (tool: Extract<Gesture, { kind: "tool" }>, event: PointerEvent) => {
      const tile = pickTile(event.clientX, event.clientY)
      if (!tile) {
        return
      }
      const key = `${tile.x}:${tile.y}`
      if (key === tool.tileKey) {
        return
      }
      tool.tileKey = key
      propsRef.current.onTileDrag(tile)
      if (hoverKey !== key) {
        hoverKey = key
        propsRef.current.onHover(tile)
      }
    }

    const onPointerMove = (event: PointerEvent) => {
      const previous = pointers.get(event.pointerId)
      if (tooltipRef.current) {
        const rect = container.getBoundingClientRect()
        tooltipRef.current.style.transform = `translate(${event.clientX - rect.left + 16}px, ${
          event.clientY - rect.top + 18
        }px)`
      }
      if (!previous) {
        if (event.pointerType === "mouse") {
          hoverClient = [event.clientX, event.clientY]
        }
        return
      }
      const last: [number, number] = [previous.x, previous.y]
      const now: [number, number] = [event.clientX, event.clientY]
      previous.x = event.clientX
      previous.y = event.clientY
      hoverClient = event.pointerType === "mouse" ? now : null

      if (!gesture) {
        return
      }
      switch (gesture.kind) {
        case "pending": {
          if (gesture.pointerId !== event.pointerId) {
            return
          }
          const slop = event.pointerType === "mouse" ? CLICK_SLOP_MOUSE : CLICK_SLOP_TOUCH
          if (Math.hypot(event.clientX - gesture.startX, event.clientY - gesture.startY) < slop) {
            return
          }
          panBetween([gesture.startX, gesture.startY], now)
          gesture = { kind: "pan", pointerId: event.pointerId }
          return
        }
        case "pan":
          if (gesture.pointerId === event.pointerId) {
            panBetween(last, now)
          }
          return
        case "orbit":
          if (gesture.pointerId === event.pointerId) {
            orbitBy(now[0] - last[0], now[1] - last[1])
          }
          return
        case "toolPending": {
          if (gesture.pointerId !== event.pointerId) {
            return
          }
          if (
            Math.hypot(event.clientX - gesture.startX, event.clientY - gesture.startY) <
            CLICK_SLOP_TOUCH
          ) {
            return
          }
          const start = gesture.tile
          const tool = {
            kind: "tool" as const,
            pointerId: event.pointerId,
            tileKey: `${start.x}:${start.y}`,
          }
          gesture = tool
          propsRef.current.onTileDown(start)
          dragToolTo(tool, event)
          return
        }
        case "tool":
          if (gesture.pointerId === event.pointerId) {
            dragToolTo(gesture, event)
          }
          return
        case "multi": {
          if (pointers.size < 2) {
            return
          }
          const next = twoPointerState()
          if (gesture.mode === "undecided") {
            // Fingers dragged together vertically (spacing and angle steady) tilt the camera,
            // like map apps; anything else pans, pinches and twists.
            const dx = next.mid[0] - gesture.start.mid[0]
            const dy = next.mid[1] - gesture.start.mid[1]
            const spread = Math.abs(next.distance - gesture.start.distance)
            if (Math.max(Math.hypot(dx, dy), spread) < MULTI_TOUCH_DECISION_PX) {
              return
            }
            const steady =
              spread < MULTI_TOUCH_DECISION_PX * 0.6 &&
              Math.abs(next.angle - gesture.start.angle) < 0.12
            gesture.mode = steady && Math.abs(dy) > Math.abs(dx) * 1.5 ? "tilt" : "transform"
          }
          if (gesture.mode === "tilt") {
            bridge?.orbit_by_degrees(0, (next.mid[1] - gesture.mid[1]) * TILT_DEGREES_PER_PIXEL)
            cameraDirtyRef.current = true
          } else {
            panBetween(gesture.mid, next.mid)
            zoomAt(next.distance / gesture.distance, next.mid[0], next.mid[1])
            let turn = next.angle - gesture.angle
            if (turn > Math.PI) {
              turn -= Math.PI * 2
            }
            if (turn < -Math.PI) {
              turn += Math.PI * 2
            }
            bridge?.orbit_by_degrees((-turn * 180) / Math.PI, 0)
          }
          gesture.mid = next.mid
          gesture.distance = next.distance
          gesture.angle = next.angle
          return
        }
      }
    }

    const releasePointer = (event: PointerEvent, commit: boolean) => {
      if (!pointers.has(event.pointerId)) {
        return
      }
      pointers.delete(event.pointerId)
      if (canvas.hasPointerCapture(event.pointerId)) {
        canvas.releasePointerCapture(event.pointerId)
      }
      if (!gesture) {
        return
      }

      if (gesture.kind === "multi") {
        const remaining = [...pointers.keys()]
        const survivor = remaining.length === 1 ? remaining[0] : undefined
        gesture = survivor === undefined ? null : { kind: "pan", pointerId: survivor }
        return
      }
      if ("pointerId" in gesture && gesture.pointerId !== event.pointerId) {
        return
      }

      if (gesture.kind === "pending" && commit) {
        propsRef.current.onPick(resolvePick(event.clientX, event.clientY))
      }
      if (gesture.kind === "toolPending" && commit) {
        // A single-finger tap: confirm the tool on its start tile and finish immediately.
        propsRef.current.onTileDown(gesture.tile)
        propsRef.current.onToolGestureEnd(true)
      }
      if (gesture.kind === "tool") {
        propsRef.current.onToolGestureEnd(commit)
      }
      gesture = null
    }

    const onPointerUp = (event: PointerEvent) => releasePointer(event, true)
    const onPointerCancel = (event: PointerEvent) => releasePointer(event, false)
    // Losing capture without a pointerup (another element or the OS took the pointer) cancels.
    const onLostPointerCapture = (event: PointerEvent) => releasePointer(event, false)

    const onPointerLeave = (event: PointerEvent) => {
      if (event.pointerType !== "mouse" || pointers.size > 0) {
        return
      }
      hoverClient = null
      if (hoverKey !== "") {
        hoverKey = ""
        propsRef.current.onHover(null)
      }
    }

    const onWheel = (event: WheelEvent) => {
      event.preventDefault()
      const scale = WHEEL_SCALE_BY_DELTA_MODE[event.deltaMode] ?? 0.0015
      zoomAt(Math.exp(-event.deltaY * scale), event.clientX, event.clientY)
    }

    const onContextMenu = (event: Event) => event.preventDefault()

    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey
      ) {
        return
      }
      const rect = canvas.getBoundingClientRect()
      const center: [number, number] = [rect.left + rect.width / 2, rect.top + rect.height / 2]
      const step = Math.min(rect.width, rect.height) * 0.08
      const pan = (dx: number, dy: number) => panBetween(center, [center[0] + dx, center[1] + dy])
      switch (event.key) {
        case "Escape":
          // Cancels the construction gesture in progress; nothing is committed or spent.
          if (gesture?.kind === "tool" || gesture?.kind === "toolPending") {
            endToolGesture(false)
            gesture = null
          }
          break
        case "ArrowLeft":
        case "a":
        case "A":
          pan(step, 0)
          break
        case "ArrowRight":
        case "d":
        case "D":
          pan(-step, 0)
          break
        case "ArrowUp":
        case "w":
        case "W":
          pan(0, step)
          break
        case "ArrowDown":
        case "s":
        case "S":
          pan(0, -step)
          break
        case "q":
        case "Q":
          bridge?.orbit_by_degrees(-15, 0)
          cameraDirtyRef.current = true
          break
        case "e":
        case "E":
          bridge?.orbit_by_degrees(15, 0)
          cameraDirtyRef.current = true
          break
        case "r":
        case "R":
          bridge?.orbit_by_degrees(0, 6)
          cameraDirtyRef.current = true
          break
        case "f":
        case "F":
          bridge?.orbit_by_degrees(0, -6)
          cameraDirtyRef.current = true
          break
        case "+":
        case "=":
          zoomAt(1.2, ...center)
          break
        case "-":
        case "_":
          zoomAt(1 / 1.2, ...center)
          break
        case "Home":
          bridge?.reset()
          cameraDirtyRef.current = true
          break
        default:
          return
      }
      event.preventDefault()
    }

    const installDebugHook = () => {
      window.__zooParkView = {
        ready: true,
        camera: () =>
          camera && {
            yawDegrees: camera.yawDegrees,
            pitchDegrees: camera.pitchDegrees,
            zoom: camera.zoom,
            target: camera.target,
          },
        projectWorld: (point) => {
          if (!camera) {
            return null
          }
          const rect = canvas.getBoundingClientRect()
          const projected = createWorldProjector(camera, viewport)(point)
          return {
            x: rect.left + projected.x,
            y: rect.top + projected.y,
            visible: projected.visible,
          }
        },
        parkFootprintInFrame: () => {
          if (!camera) {
            return false
          }
          const project = createWorldProjector(camera, viewport)
          const { width, height } = propsRef.current.snapshot
          const corners: [number, number][] = [
            [0, 0],
            [width, 0],
            [0, height],
            [width, height],
          ]
          return corners.every(([x, z]) => {
            const point = project([x, 0, z])
            return (
              point.visible &&
              point.x >= 0 &&
              point.y >= 0 &&
              point.x <= viewport.width &&
              point.y <= viewport.height
            )
          })
        },
        nodeCount: () => nodeCount,
        drawCount: () => drawCount,
        resources: () =>
          work && gpu
            ? {
                objects: work.liveObjectCount,
                geometries: work.liveGeometryCount,
                materials: work.liveMaterialCount,
                gpuGeometries: gpu.info.memory.geometries,
                gpuTextures: gpu.info.memory.textures,
                gpuPrograms: gpu.info.programs?.length ?? 0,
              }
            : null,
        effects: () =>
          effects && {
            live: { ...effects.state.live },
            started: { ...effects.state.started },
            reducedMotion: reducedMotion.matches,
            liveAtlases: work?.liveEffectAtlasCount ?? 0,
          },
      }
    }

    const onContextLost = (event: Event) => {
      event.preventDefault()
      failClosed(new Error("WebGL context lost"))
    }

    // The scenery pieces are required content: a missing or mismatched asset fails the view closed.
    // The particle effects are cosmetic: without them the park still plays, so a load failure only
    // warns and the view runs effect-free.
    void Promise.all([
      initScene(),
      loadSceneryKit().catch((error: unknown) => {
        throw new Error(
          `Zoo scenery assets failed to load (${error instanceof Error ? error.message : String(error)})`,
        )
      }),
      loadEffectsKit().catch((error: unknown) => {
        console.warn("Zoo particle effects failed to load; running without them", error)
        return null
      }),
    ])
      .then(([, loaded, effectsKit]) => {
        if (disposed) {
          return
        }
        kit = loaded
        effects = effectsKit && new ParkEffects(effectsKit)
        bridge = new ParkCameraBridge(parkWidth, parkHeight)
        bridgeRef.current = bridge
        // A fresh rig (new park or new map size) applies the latest request, such as a new
        // scenario's opening focus that arrived before the rig existed.
        appliedCameraRef.current = -1
        applyCameraRequest()
        renderer = createThreeSceneRenderer(canvas, { background: BACKGROUND, antialias: true })
        // The loader hook is the shared renderer's public route to its Three.js WebGLRenderer.
        renderer.configureTextureLoader({
          detectSupport: (webgl: GpuRenderer) => {
            gpu = webgl
          },
        })
        lifetime.mounts += 1
        window.__zooRendererLifetime = lifetime
        resizeObserver.observe(container)
        resize()
        canvas.addEventListener("pointerdown", onPointerDown)
        canvas.addEventListener("pointermove", onPointerMove)
        canvas.addEventListener("pointerup", onPointerUp)
        canvas.addEventListener("pointercancel", onPointerCancel)
        canvas.addEventListener("lostpointercapture", onLostPointerCapture)
        canvas.addEventListener("pointerleave", onPointerLeave)
        canvas.addEventListener("wheel", onWheel, { passive: false })
        canvas.addEventListener("contextmenu", onContextMenu)
        canvas.addEventListener("webglcontextlost", onContextLost)
        window.addEventListener("keydown", onKeyDown)
        installDebugHook()
        setStatus("ready")
        frameHandle = window.requestAnimationFrame(renderFrame)
      })
      .catch(failClosed)

    return () => {
      disposed = true
      window.cancelAnimationFrame(frameHandle)
      resizeObserver.disconnect()
      canvas.removeEventListener("pointerdown", onPointerDown)
      canvas.removeEventListener("pointermove", onPointerMove)
      canvas.removeEventListener("pointerup", onPointerUp)
      canvas.removeEventListener("pointercancel", onPointerCancel)
      canvas.removeEventListener("lostpointercapture", onLostPointerCapture)
      canvas.removeEventListener("pointerleave", onPointerLeave)
      canvas.removeEventListener("wheel", onWheel)
      canvas.removeEventListener("contextmenu", onContextMenu)
      canvas.removeEventListener("webglcontextlost", onContextLost)
      window.removeEventListener("keydown", onKeyDown)
      delete window.__zooParkView
      release()
      bridge?.free()
      bridgeRef.current = null
    }
  }, [parkWidth, parkHeight, applyCameraRequest])

  // Camera requests from the HUD (reset) and from starting a scenario (opening focus).
  useEffect(() => {
    applyCameraRequest()
  }, [props.camera.token, applyCameraRequest])

  return (
    <div ref={containerRef} className="park-view" data-renderer={status}>
      {/* A new map size gets a fresh canvas: a released WebGL context cannot be reused. */}
      <canvas
        key={`${parkWidth}x${parkHeight}`}
        ref={canvasRef}
        className="park-canvas"
        tabIndex={0}
        aria-label="Zoo park 3D view. Drag to pan, right-drag to rotate, scroll to zoom."
      />
      {props.tooltip && (
        <div ref={tooltipRef} className="park-tooltip bevel">
          {props.tooltip}
        </div>
      )}
      {status === "failed" && (
        <div className="park-view-error">The 3D renderer stopped in this browser.</div>
      )}
    </div>
  )
}
