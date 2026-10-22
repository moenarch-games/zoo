import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer"

import type {
  Animal,
  FenceSegment,
  FenceSide,
  GateEvaluation,
  Guest,
  Habitat,
  PlacementEvaluation,
  Point,
  Snapshot,
  SpeciesKey,
  Tile,
  Tool,
  WorkZoneEvaluation,
  ZoneBounds,
} from "./game-types"
import { NO_ZONE_COLOR, zoneColor } from "./zone-colors"

// Zoo scene composition: maps authoritative zoo-core state onto shared 3d-lab renderer nodes.
// World convention (shared with zoo-scene): park tile (x, y) covers [x, x + 1] × [y, y + 1] on
// the y = 0 ground plane, +y is up. This module owns art direction only; it makes no rule
// decisions and performs no camera or projection math.

export type Vec3 = [number, number, number]
type Hex = `#${string}`
type Quat = [number, number, number, number]
type Triangle = [Vec3, Vec3, Vec3]

type MeshGeometry = Extract<RendererSceneNode["geometry"], { kind: "mesh" }>

export type SceneOverlay = {
  tool: Tool
  hoveredTile: Point | null
  ghostTiles: Point[]
  placement: PlacementEvaluation | null
  selectedHabitatId: number | null
  selectedGuestId: number | null
  selectedDepot: boolean
  /** Rust's evaluation of the fence segment under the pointer while moving a gate. */
  gatePreview: GateEvaluation | null
  /** Work zones are drawn only while the Zones tool is active or a zone is selected. */
  zonesVisible: boolean
  selectedZoneId: number | null
  /** Rust's evaluation of the zone being drawn, resized, or moved. */
  zonePreview: WorkZoneEvaluation | null
}

/** Resolves the presented (smoothed) ground position for a moving actor. */
export type ActorResolver = (key: string, x: number, z: number) => [number, number]

export type SceneFrameInput = {
  snapshot: Snapshot
  overlay: SceneOverlay
  timeSeconds: number
  animate: boolean
  resolveActor: ActorResolver
}

/** Screen-pickable anchors for entities that do not map 1:1 to a clicked ground tile. */
export type PickAnchor =
  | { kind: "guest"; id: number; point: Vec3; radius: number }
  | { kind: "animal"; habitatId: number; point: Vec3; radius: number }
  | { kind: "concession"; id: number; tile: Point; point: Vec3; radius: number }
  | { kind: "depot"; point: Vec3; radius: number }

export type SceneFrame = {
  nodes: RendererSceneNode[]
  anchors: PickAnchor[]
}

const IDENTITY: Quat = [0, 0, 0, 1]

function yawQuaternion(yaw: number): Quat {
  return [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)]
}

function multiplyQuaternion(a: Quat, b: Quat): Quat {
  return [
    a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
    a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
    a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
    a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
  ]
}

function axisQuaternion(axis: "x" | "z", angle: number): Quat {
  const s = Math.sin(angle / 2)
  return axis === "x" ? [s, 0, 0, Math.cos(angle / 2)] : [0, 0, s, Math.cos(angle / 2)]
}

/** Deterministic hash → [0, 1) used for decorative variation. */
function hash(...values: number[]) {
  let h = 2166136261
  for (const value of values) {
    h ^= Math.floor(value * 997) | 0
    h = Math.imul(h, 16777619)
    h ^= h >>> 13
  }
  return ((h >>> 0) % 100_000) / 100_000
}

function shade(color: Hex, amount: number): Hex {
  const value = Number.parseInt(color.slice(1), 16)
  const channel = (shift: number) => {
    const c = (value >> shift) & 0xff
    const next = amount >= 0 ? c + (255 - c) * amount : c * (1 + amount)
    return Math.max(0, Math.min(255, Math.round(next)))
  }
  const rgb = (channel(16) << 16) | (channel(8) << 8) | channel(0)
  return `#${rgb.toString(16).padStart(6, "0")}`
}

// ---------------------------------------------------------------------------------------------
// Faceted meshes. Each triangle owns its vertices so computed normals stay flat (low-poly look).

function facetedMesh(resourceKey: string, triangles: Triangle[]): MeshGeometry {
  const center = triangles
    .flat()
    .reduce<Vec3>((sum, p) => [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]], [0, 0, 0])
    .map((value) => value / (triangles.length * 3)) as Vec3
  const positions: Vec3[] = []
  const indices: number[] = []
  for (const [a, b, c] of triangles) {
    const ab: Vec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
    const ac: Vec3 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]]
    const normal: Vec3 = [
      ab[1] * ac[2] - ab[2] * ac[1],
      ab[2] * ac[0] - ab[0] * ac[2],
      ab[0] * ac[1] - ab[1] * ac[0],
    ]
    const outward: Vec3 = [
      (a[0] + b[0] + c[0]) / 3 - center[0],
      (a[1] + b[1] + c[1]) / 3 - center[1],
      (a[2] + b[2] + c[2]) / 3 - center[2],
    ]
    const facesOut = normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2] >= 0
    const base = positions.length
    positions.push(a, ...(facesOut ? [b, c] : [c, b]))
    indices.push(base, base + 1, base + 2)
  }
  return { kind: "mesh", resourceKey, positions, indices }
}

/** Gable roof prism: 1 × 1 footprint centered on the origin, ridge along X, height 1. */
const GABLE_ROOF = facetedMesh("zoo:gable-roof:v1", [
  [
    [-0.5, 0, -0.5],
    [0.5, 0, -0.5],
    [0.5, 1, 0],
  ],
  [
    [-0.5, 0, -0.5],
    [0.5, 1, 0],
    [-0.5, 1, 0],
  ],
  [
    [-0.5, 0, 0.5],
    [0.5, 1, 0],
    [0.5, 0, 0.5],
  ],
  [
    [-0.5, 0, 0.5],
    [-0.5, 1, 0],
    [0.5, 1, 0],
  ],
  [
    [-0.5, 0, -0.5],
    [-0.5, 1, 0],
    [-0.5, 0, 0.5],
  ],
  [
    [0.5, 0, -0.5],
    [0.5, 0, 0.5],
    [0.5, 1, 0],
  ],
  [
    [-0.5, 0, -0.5],
    [-0.5, 0, 0.5],
    [0.5, 0, 0.5],
  ],
  [
    [-0.5, 0, -0.5],
    [0.5, 0, 0.5],
    [0.5, 0, -0.5],
  ],
])

function coneTriangles(segments: number): Triangle[] {
  const triangles: Triangle[] = []
  for (let i = 0; i < segments; i += 1) {
    const a = (i / segments) * Math.PI * 2
    const b = ((i + 1) / segments) * Math.PI * 2
    const pa: Vec3 = [Math.cos(a) * 0.5, 0, Math.sin(a) * 0.5]
    const pb: Vec3 = [Math.cos(b) * 0.5, 0, Math.sin(b) * 0.5]
    triangles.push([pa, pb, [0, 1, 0]], [pa, [0, 0, 0], pb])
  }
  return triangles
}

/** Unit cone: base diameter 1 at y = 0, apex at y = 1. */
const CONE = facetedMesh("zoo:cone7:v1", coneTriangles(7))

/** Low-poly rock/bush blob: an irregular octahedron-ish hull, unit radius. */
const BLOB = facetedMesh(
  "zoo:blob:v1",
  (() => {
    const top: Vec3 = [0, 0.8, 0]
    const bottom: Vec3 = [0, -0.3, 0]
    const ring: Vec3[] = [0, 1, 2, 3, 4, 5].map((i) => {
      const angle = (i / 6) * Math.PI * 2 + 0.3
      const radius = i % 2 === 0 ? 1 : 0.82
      return [Math.cos(angle) * radius, 0.12 * (i % 3), Math.sin(angle) * radius] as Vec3
    })
    const triangles: Triangle[] = []
    ring.forEach((a, i) => {
      const b = ring[(i + 1) % ring.length] ?? a
      triangles.push([a, b, top], [a, bottom, b])
    })
    return triangles
  })(),
)

// ---------------------------------------------------------------------------------------------
// Node helpers

type PartOptions = { opacity?: number; rotation?: Quat; scale?: Vec3 }

function node(
  id: string,
  translation: Vec3,
  geometry: RendererSceneNode["geometry"],
  color: Hex,
  options: PartOptions = {},
): RendererSceneNode {
  const transform: { translation: Vec3; rotationQuaternion?: Quat; scale?: Vec3 } = { translation }
  if (options.rotation && options.rotation !== IDENTITY) {
    transform.rotationQuaternion = options.rotation
  }
  if (options.scale) {
    transform.scale = options.scale
  }
  const result: RendererSceneNode = { id, transform, geometry, color }
  if (options.opacity !== undefined && options.opacity < 1) {
    result.opacity = options.opacity
  }
  return result
}

const boxGeometry = (size: Vec3) => ({ kind: "box", size }) as const
const cylinderGeometry = (radius: number, height: number) =>
  ({ kind: "cylinder", radius, height }) as const
const sphereGeometry = (radius: number) => ({ kind: "sphere", radius }) as const

/** Places parts in a model-local frame (x right, y up, z forward) at a ground origin and yaw. */
function model(prefix: string, origin: { x: number; z: number }, yaw = 0, lift = 0) {
  const rotation = yaw === 0 ? IDENTITY : yawQuaternion(yaw)
  const cosine = Math.cos(yaw)
  const sine = Math.sin(yaw)
  const at = ([x, y, z]: Vec3): Vec3 => [
    origin.x + x * cosine + z * sine,
    y + lift,
    origin.z - x * sine + z * cosine,
  ]
  const localRotation = (extra?: Quat) => (extra ? multiplyQuaternion(rotation, extra) : rotation)
  return {
    at,
    box: (part: string, local: Vec3, size: Vec3, color: Hex, options: PartOptions = {}) =>
      node(`${prefix}:${part}`, at(local), boxGeometry(size), color, {
        ...options,
        rotation: localRotation(options.rotation),
      }),
    cylinder: (
      part: string,
      local: Vec3,
      radius: number,
      height: number,
      color: Hex,
      options: PartOptions = {},
    ) =>
      node(`${prefix}:${part}`, at(local), cylinderGeometry(radius, height), color, {
        ...options,
        rotation: localRotation(options.rotation),
      }),
    sphere: (part: string, local: Vec3, radius: number, color: Hex, options: PartOptions = {}) =>
      node(`${prefix}:${part}`, at(local), sphereGeometry(radius), color, {
        ...options,
        rotation: localRotation(options.rotation),
      }),
    mesh: (
      part: string,
      geometry: MeshGeometry,
      local: Vec3,
      scale: Vec3,
      color: Hex,
      options: PartOptions = {},
    ) =>
      node(`${prefix}:${part}`, at(local), geometry, color, {
        ...options,
        scale,
        rotation: localRotation(options.rotation),
      }),
  }
}

function blobShadow(id: string, x: number, z: number, radius: number, opacity = 0.22) {
  return node(id, [x, 0.012, z], cylinderGeometry(radius, 0.01), "#1f3b24", { opacity })
}

// ---------------------------------------------------------------------------------------------
// Palette

const GRASS_A: Hex = "#7fbb5a"
const GRASS_B: Hex = "#78b456"
const OUTER_GRASS: Hex = "#6aa650"
const SOIL: Hex = "#8a6a45"
const PATH: Hex = "#dcc59a"
const PATH_EDGE: Hex = "#c2a979"
const ENTRANCE_PLAZA: Hex = "#d4b98a"

const HABITAT_GROUND: Record<SpeciesKey, Hex> = {
  empty: "#a58f62",
  capybara: "#7aa75a",
  flamingo: "#9cc7a6",
  zebra: "#d8c27e",
  giraffe: "#d3b06f",
  elephant: "#c9a46e",
  penguin: "#dfeaf0",
}

// ---------------------------------------------------------------------------------------------
// Terrain

function tileColor(tile: Tile, habitatSpecies: Map<number, SpeciesKey | null>): Hex {
  switch (tile.kind) {
    case "path":
      return PATH
    case "entrance":
      return ENTRANCE_PLAZA
    case "concession":
      return PATH_EDGE
    case "habitat": {
      const species =
        tile.habitat_id === null ? null : (habitatSpecies.get(tile.habitat_id) ?? null)
      const base = HABITAT_GROUND[species ?? "empty"]
      if (!base) {
        throw new Error(`Unsupported habitat presentation: ${species}`)
      }
      return hash(tile.x, tile.y, 7) > 0.5 ? base : shade(base, -0.04)
    }
    case "grass":
      return (tile.x + tile.y) % 2 === 0 ? GRASS_A : GRASS_B
  }
}

function terrainNodes(snapshot: Snapshot): RendererSceneNode[] {
  const habitatSpecies = new Map(snapshot.habitats.map((h) => [h.id, h.species]))
  const nodes: RendererSceneNode[] = []
  const span = Math.max(snapshot.width, snapshot.height)
  const cx = snapshot.width / 2
  const cz = snapshot.height / 2

  // Surrounding meadow sits slightly below the park plateau; the plateau's soil edge is visible.
  nodes.push(
    node("world:meadow", [cx, -0.16, cz], boxGeometry([span * 7, 0.2, span * 7]), OUTER_GRASS),
    node(
      "world:plateau",
      [cx, -0.09, cz],
      boxGeometry([snapshot.width + 0.08, 0.12, snapshot.height + 0.08]),
      SOIL,
    ),
  )

  for (const tile of snapshot.tiles) {
    const raised = tile.kind === "path" || tile.kind === "entrance" || tile.kind === "concession"
    const height = raised ? 0.08 : 0.06
    nodes.push(
      node(
        `tile:${tile.x}:${tile.y}`,
        [tile.x + 0.5, -0.06 + height / 2 + (raised ? 0.01 : 0), tile.y + 0.5],
        boxGeometry([1, height, 1]),
        tileColor(tile, habitatSpecies),
      ),
    )
  }

  // Approach road leading from the gate out into the world.
  const side = entranceSide(snapshot)
  const outward = sideNormal(side)
  const along = side === "west" || side === "east" ? "x" : "z"
  for (let step = 1; step <= Math.ceil(span * 1.8); step += 1) {
    const x = snapshot.entrance.x + 0.5 + outward.x * step
    const z = snapshot.entrance.y + 0.5 + outward.z * step
    nodes.push(
      node(
        `world:approach:${step}`,
        [x, -0.045, z],
        boxGeometry(along === "x" ? [1, 0.03, 1.4] : [1.4, 0.03, 1]),
        step % 2 === 0 ? PATH : shade(PATH, -0.03),
      ),
    )
  }
  return nodes
}

// ---------------------------------------------------------------------------------------------
// Scenery outside the park: deterministic trees, bushes and rocks.

function sceneryNodes(snapshot: Snapshot): RendererSceneNode[] {
  const nodes: RendererSceneNode[] = []
  const side = entranceSide(snapshot)
  const outward = sideNormal(side)
  const entrance = { x: snapshot.entrance.x + 0.5, z: snapshot.entrance.y + 0.5 }
  const reach = 9
  for (let gx = -reach; gx < snapshot.width + reach; gx += 1) {
    for (let gz = -reach; gz < snapshot.height + reach; gz += 1) {
      const inside = gx >= -1 && gz >= -1 && gx <= snapshot.width && gz <= snapshot.height
      if (inside) {
        continue
      }
      const roll = hash(gx, gz, 1)
      const edgeDistance = Math.max(-gx - 1, gx - snapshot.width, -gz - 1, gz - snapshot.height)
      const density = edgeDistance < 2 ? 0.18 : 0.34
      if (roll > density) {
        continue
      }
      const x = gx + 0.5 + (hash(gx, gz, 2) - 0.5) * 0.7
      const z = gz + 0.5 + (hash(gx, gz, 3) - 0.5) * 0.7
      // Keep the approach road and a little breathing room around it clear.
      const alongRoad = (x - entrance.x) * outward.x + (z - entrance.z) * outward.z
      const acrossRoad = Math.abs((x - entrance.x) * outward.z - (z - entrance.z) * outward.x)
      if (alongRoad > -0.5 && acrossRoad < 2.2) {
        continue
      }
      const variant = hash(gx, gz, 4)
      const scale = 0.75 + hash(gx, gz, 5) * 0.6
      const id = `scenery:${gx}:${gz}`
      if (variant < 0.45) {
        nodes.push(...roundTree(id, x, z, scale, hash(gx, gz, 6)))
      } else if (variant < 0.8) {
        nodes.push(...pineTree(id, x, z, scale))
      } else if (variant < 0.93) {
        nodes.push(...bush(id, x, z, scale))
      } else {
        nodes.push(rock(id, x, z, scale, hash(gx, gz, 7)))
      }
    }
  }
  // Sparse outer woodland (trees only) so framings that fit a narrow viewport by width, such as
  // phones in portrait, don't show a bare meadow above and below the park.
  const farReach = 22
  for (let gx = -farReach; gx < snapshot.width + farReach; gx += 1) {
    for (let gz = -farReach; gz < snapshot.height + farReach; gz += 1) {
      const nearRing =
        gx >= -reach && gz >= -reach && gx < snapshot.width + reach && gz < snapshot.height + reach
      if (nearRing || hash(gx, gz, 11) > 0.09) {
        continue
      }
      const x = gx + 0.5 + (hash(gx, gz, 12) - 0.5) * 0.8
      const z = gz + 0.5 + (hash(gx, gz, 13) - 0.5) * 0.8
      const alongRoad = (x - entrance.x) * outward.x + (z - entrance.z) * outward.z
      const acrossRoad = Math.abs((x - entrance.x) * outward.z - (z - entrance.z) * outward.x)
      if (alongRoad > -0.5 && acrossRoad < 2.2) {
        continue
      }
      const scale = 1 + hash(gx, gz, 14) * 0.6
      const id = `scenery:far:${gx}:${gz}`
      if (hash(gx, gz, 15) < 0.5) {
        nodes.push(...roundTree(id, x, z, scale, hash(gx, gz, 16)))
      } else {
        nodes.push(...pineTree(id, x, z, scale))
      }
    }
  }
  return nodes
}

function roundTree(id: string, x: number, z: number, scale: number, tint: number) {
  const leaf = shade("#4f9444", (tint - 0.5) * 0.25)
  const m = model(id, { x, z })
  return [
    blobShadow(`${id}:shadow`, x, z, 0.45 * scale),
    m.cylinder("trunk", [0, 0.35 * scale, 0], 0.07 * scale, 0.7 * scale, "#7a5536"),
    m.sphere("crown", [0, 0.95 * scale, 0], 0.42 * scale, leaf),
    m.sphere(
      "crown-top",
      [0.12 * scale, 1.25 * scale, -0.05 * scale],
      0.26 * scale,
      shade(leaf, 0.08),
    ),
  ]
}

function pineTree(id: string, x: number, z: number, scale: number) {
  const m = model(id, { x, z })
  return [
    blobShadow(`${id}:shadow`, x, z, 0.38 * scale),
    m.cylinder("trunk", [0, 0.18 * scale, 0], 0.06 * scale, 0.36 * scale, "#6b4a30"),
    m.mesh("lower", CONE, [0, 0.3 * scale, 0], [0.8 * scale, 0.9 * scale, 0.8 * scale], "#2f7449"),
    m.mesh(
      "upper",
      CONE,
      [0, 0.8 * scale, 0],
      [0.58 * scale, 0.75 * scale, 0.58 * scale],
      "#37825a",
    ),
  ]
}

function bush(id: string, x: number, z: number, scale: number) {
  const m = model(id, { x, z }, hash(x, z) * 6)
  return [
    m.mesh(
      "body",
      BLOB,
      [0, 0.12 * scale, 0],
      [0.32 * scale, 0.32 * scale, 0.32 * scale],
      "#5a9c48",
    ),
    m.mesh(
      "side",
      BLOB,
      [0.22 * scale, 0.08 * scale, 0.1 * scale],
      [0.2 * scale, 0.22 * scale, 0.2 * scale],
      "#66a852",
    ),
  ]
}

function rock(id: string, x: number, z: number, scale: number, turn: number) {
  return model(id, { x, z }, turn * 6).mesh(
    "stone",
    BLOB,
    [0, 0.04, 0],
    [0.28 * scale, 0.22 * scale, 0.22 * scale],
    "#9a9a8e",
  )
}

// ---------------------------------------------------------------------------------------------
// Service gates: a gap in the habitat fence framed by two coloured posts and a threshold.

function gateMarker(
  id: string,
  segment: FenceSegment,
  color: Hex,
  options: { highlighted: boolean; opacity?: number },
): RendererSceneNode[] {
  const [start, end] = fenceEndpoints(segment.x, segment.y, segment.side)
  const alongX = segment.side === "north" || segment.side === "south"
  const height = options.highlighted ? 0.85 : 0.62
  const opacity = options.opacity ?? 1
  const nodes = [start, end].map((point, index) =>
    node(
      `${id}:post:${index}`,
      [point[0], height / 2, point[2]],
      boxGeometry([0.12, height, 0.12]),
      color,
      { opacity },
    ),
  )
  const cx = (start[0] + end[0]) / 2
  const cz = (start[2] + end[2]) / 2
  nodes.push(
    node(
      `${id}:threshold`,
      [cx, 0.035, cz],
      boxGeometry(alongX ? [0.9, 0.03, 0.22] : [0.22, 0.03, 0.9]),
      color,
      { opacity: opacity * 0.85 },
    ),
  )
  if (options.highlighted) {
    nodes.push(
      node(
        `${id}:lintel`,
        [cx, height, cz],
        boxGeometry(alongX ? [1.05, 0.08, 0.1] : [0.1, 0.08, 1.05]),
        color,
        { opacity },
      ),
    )
  }
  return nodes
}

function gateNodes(snapshot: Snapshot, overlay: SceneOverlay): RendererSceneNode[] {
  const nodes: RendererSceneNode[] = []
  for (const habitat of snapshot.habitats) {
    const gate = habitat.service_gate
    const selected = habitat.id === overlay.selectedHabitatId
    let color: Hex = selected ? "#ffb347" : "#d98a2b"
    if (!gate.reachable) {
      color = "#d0584a"
    }
    if (overlay.zonesVisible) {
      color = habitat.work_zone_id === null ? NO_ZONE_COLOR : zoneColor(habitat.work_zone_id)
      if (habitat.work_zone_id === null) {
        nodes.push(...noZoneMarker(`gate:${habitat.id}:no-zone`, gate))
      }
    }
    nodes.push(...gateMarker(`gate:${habitat.id}`, gate, color, { highlighted: selected }))
    if (selected && gate.access) {
      nodes.push(
        node(
          `gate:${habitat.id}:access`,
          [gate.access.x + 0.5, 0.045, gate.access.y + 0.5],
          boxGeometry([0.9, 0.02, 0.9]),
          color,
          { opacity: 0.3 },
        ),
      )
    }
  }
  const preview = overlay.gatePreview
  if (preview?.segment) {
    nodes.push(
      ...gateMarker("gate:preview", preview.segment, preview.ok ? "#9fe07a" : "#e0604f", {
        highlighted: true,
        opacity: 0.75,
      }),
    )
  }
  return nodes
}

/** A red flag above a service gate that no work zone covers ("No zone"). */
function noZoneMarker(id: string, segment: FenceSegment): RendererSceneNode[] {
  const [start, end] = fenceEndpoints(segment.x, segment.y, segment.side)
  const cx = (start[0] + end[0]) / 2
  const cz = (start[2] + end[2]) / 2
  return [
    node(`${id}:pole`, [cx, 0.75, cz], boxGeometry([0.05, 1.5, 0.05]), "#5b4636"),
    node(`${id}:flag`, [cx, 1.38, cz], sphereGeometry(0.18), NO_ZONE_COLOR),
  ]
}

// ---------------------------------------------------------------------------------------------
// Keeper work zones: a translucent fill plus an outline per map-aligned rectangle.

function zoneRect(id: string, bounds: ZoneBounds, color: Hex, fill: number): RendererSceneNode[] {
  const { x, y, width, height } = bounds
  const cx = x + width / 2
  const cz = y + height / 2
  const edge = 0.08
  const lift = 0.06
  return [
    node(`${id}:fill`, [cx, 0.04, cz], boxGeometry([width, 0.01, height]), color, {
      opacity: fill,
    }),
    node(`${id}:n`, [cx, lift, y + edge / 2], boxGeometry([width, 0.03, edge]), color),
    node(`${id}:s`, [cx, lift, y + height - edge / 2], boxGeometry([width, 0.03, edge]), color),
    node(`${id}:w`, [x + edge / 2, lift, cz], boxGeometry([edge, 0.03, height]), color),
    node(`${id}:e`, [x + width - edge / 2, lift, cz], boxGeometry([edge, 0.03, height]), color),
  ]
}

function zoneNodes(snapshot: Snapshot, overlay: SceneOverlay): RendererSceneNode[] {
  const nodes: RendererSceneNode[] = []
  if (!overlay.zonesVisible) {
    return nodes
  }
  for (const zone of snapshot.work_zones) {
    const selected = zone.id === overlay.selectedZoneId
    nodes.push(...zoneRect(`zone:${zone.id}`, zone, zoneColor(zone.id), selected ? 0.32 : 0.18))
  }
  const preview = overlay.zonePreview
  if (preview) {
    nodes.push(...zoneRect("zone:preview", preview.bounds, preview.ok ? "#9fe07a" : "#e0604f", 0.3))
  }
  return nodes
}

// ---------------------------------------------------------------------------------------------
// Fences

type FenceRun = { id: string; start: Vec3; end: Vec3; kind: "boundary" | "habitat" | "preview" }

function tileCorners(x: number, z: number): [Vec3, Vec3, Vec3, Vec3] {
  return [
    [x, 0, z],
    [x + 1, 0, z],
    [x + 1, 0, z + 1],
    [x, 0, z + 1],
  ]
}

function fenceEndpoints(x: number, z: number, side: FenceSide): [Vec3, Vec3] {
  const [northWest, northEast, southEast, southWest] = tileCorners(x, z)
  switch (side) {
    case "north":
      return [northWest, northEast]
    case "east":
      return [northEast, southEast]
    case "south":
      return [southWest, southEast]
    case "west":
      return [northWest, southWest]
  }
}

const pointKey = ([x, , z]: Vec3) => `${x}:${z}`
const edgeKey = (a: Vec3, b: Vec3) => [pointKey(a), pointKey(b)].sort().join("--")

export function entranceSide(snapshot: Snapshot): FenceSide {
  if (snapshot.entrance.y <= 0) {
    return "north"
  }
  if (snapshot.entrance.y >= snapshot.height - 1) {
    return "south"
  }
  if (snapshot.entrance.x <= 0) {
    return "west"
  }
  return "east"
}

function sideNormal(side: FenceSide) {
  switch (side) {
    case "north":
      return { x: 0, z: -1 }
    case "south":
      return { x: 0, z: 1 }
    case "west":
      return { x: -1, z: 0 }
    case "east":
      return { x: 1, z: 0 }
  }
}

/** Yaw that turns a model's local +z (its front) to face outward across `side`. */
function yawFacing(side: FenceSide) {
  switch (side) {
    case "south":
      return 0
    case "east":
      return Math.PI / 2
    case "north":
      return Math.PI
    case "west":
      return -Math.PI / 2
  }
}

function fenceRuns(snapshot: Snapshot, placement: PlacementEvaluation | null): FenceRun[] {
  const committed = new Map<string, FenceRun>()
  const side = entranceSide(snapshot)
  const push = (run: FenceRun) => {
    const key = edgeKey(run.start, run.end)
    const existing = committed.get(key)
    if (!existing || run.kind === "boundary") {
      committed.set(key, run)
    }
  }

  for (const habitat of snapshot.habitats) {
    const gate = habitat.service_gate
    for (const segment of habitat.fence_segments) {
      if (segment.x === gate.x && segment.y === gate.y && segment.side === gate.side) {
        // The service gate is a gap in the fence; gateNodes marks it.
        continue
      }
      const [start, end] = fenceEndpoints(segment.x, segment.y, segment.side)
      push({ id: `habitat:${habitat.id}:${edgeKey(start, end)}`, start, end, kind: "habitat" })
    }
  }

  const boundary = (x: number, z: number, edge: FenceSide) => {
    const isGate = edge === side && x === snapshot.entrance.x && z === snapshot.entrance.y
    if (isGate) {
      return
    }
    const [start, end] = fenceEndpoints(x, z, edge)
    push({ id: `boundary:${edge}:${x}:${z}`, start, end, kind: "boundary" })
  }
  for (let x = 0; x < snapshot.width; x += 1) {
    boundary(x, 0, "north")
    boundary(x, snapshot.height - 1, "south")
  }
  for (let z = 0; z < snapshot.height; z += 1) {
    boundary(0, z, "west")
    boundary(snapshot.width - 1, z, "east")
  }

  const preview = (placement?.fence_segments ?? []).map((segment, index) => {
    const [start, end] = fenceEndpoints(segment.x, segment.y, segment.side)
    return { id: `preview:${index}`, start, end, kind: "preview" } satisfies FenceRun
  })
  return [...committed.values(), ...preview]
}

function fenceNodes(snapshot: Snapshot, placement: PlacementEvaluation | null) {
  const runs = fenceRuns(snapshot, placement)
  const nodes: RendererSceneNode[] = []
  const posts = new Map<string, { point: Vec3; kind: FenceRun["kind"] }>()
  const previewColor: Hex = placement?.ok ? "#f1d66b" : "#d0584a"
  const rank = { boundary: 3, habitat: 2, preview: 1 }

  for (const run of runs) {
    const dx = run.end[0] - run.start[0]
    const dz = run.end[2] - run.start[2]
    const length = Math.hypot(dx, dz)
    const alongX = Math.abs(dx) >= Math.abs(dz)
    const cx = (run.start[0] + run.end[0]) / 2
    const cz = (run.start[2] + run.end[2]) / 2
    const opacity = run.kind === "preview" ? 0.85 : 1

    if (run.kind === "boundary") {
      // Park perimeter: low stone wall topped with a hedge-green railing.
      const wall: Vec3 = alongX ? [length, 0.2, 0.16] : [0.16, 0.2, length]
      const rail: Vec3 = alongX ? [length, 0.05, 0.06] : [0.06, 0.05, length]
      nodes.push(
        node(`fence:${run.id}:wall`, [cx, 0.1, cz], boxGeometry(wall), "#b9ad8f"),
        node(`fence:${run.id}:rail`, [cx, 0.5, cz], boxGeometry(rail), "#2f5a45"),
      )
    } else {
      const color: Hex = run.kind === "preview" ? previewColor : "#7b5a3a"
      const rail: Vec3 = alongX ? [length, 0.06, 0.05] : [0.05, 0.06, length]
      nodes.push(
        node(`fence:${run.id}:rail-low`, [cx, 0.2, cz], boxGeometry(rail), color, { opacity }),
        node(`fence:${run.id}:rail-high`, [cx, 0.42, cz], boxGeometry(rail), color, { opacity }),
      )
    }

    for (const point of [run.start, run.end]) {
      const key = pointKey(point)
      const existing = posts.get(key)
      if (!existing || rank[run.kind] > rank[existing.kind]) {
        posts.set(key, { point, kind: run.kind })
      }
    }
  }

  for (const [key, post] of [...posts.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    const [x, , z] = post.point
    if (post.kind === "boundary") {
      nodes.push(
        node(`fence:post:${key}`, [x, 0.3, z], boxGeometry([0.14, 0.6, 0.14]), "#a79c80"),
        node(`fence:post-cap:${key}`, [x, 0.62, z], boxGeometry([0.18, 0.05, 0.18]), "#8f846a"),
      )
    } else {
      const color: Hex = post.kind === "preview" ? previewColor : "#6a4c30"
      nodes.push(
        node(`fence:post:${key}`, [x, 0.26, z], cylinderGeometry(0.045, 0.52), color, {
          opacity: post.kind === "preview" ? 0.85 : 1,
        }),
      )
    }
  }

  return nodes
}

// ---------------------------------------------------------------------------------------------
// Buildings

function entranceGateNodes(snapshot: Snapshot): RendererSceneNode[] {
  const side = entranceSide(snapshot)
  const normal = sideNormal(side)
  // The gate straddles the park boundary edge of the entrance tile, front facing outward.
  const origin = {
    x: snapshot.entrance.x + 0.5 + normal.x * 0.5,
    z: snapshot.entrance.y + 0.5 + normal.z * 0.5,
  }
  const m = model("building:entrance", origin, yawFacing(side))
  const stone: Hex = "#e2d3aa"
  const trim: Hex = "#f1e8cc"
  const roof: Hex = "#b4523c"
  const nodes: RendererSceneNode[] = []

  for (const sign of [-1, 1]) {
    const s = sign < 0 ? "left" : "right"
    nodes.push(
      m.box(`pillar-${s}`, [0.62 * sign, 0.72, 0], [0.26, 1.44, 0.34], stone),
      m.box(`pillar-cap-${s}`, [0.62 * sign, 1.49, 0], [0.34, 0.08, 0.42], trim),
      m.cylinder(`flagpole-${s}`, [0.62 * sign, 1.85, 0], 0.018, 0.64, "#dcdcdc"),
      m.box(
        `flag-${s}`,
        [0.62 * sign + 0.13 * sign, 2.06, 0],
        [0.24, 0.15, 0.02],
        sign < 0 ? "#e0bf65" : "#4d9b7c",
      ),
      // Ticket booths flank the gate outside the park.
      m.box(`booth-${s}`, [1.28 * sign, 0.4, 0.58], [0.72, 0.8, 0.72], stone),
      m.mesh(`booth-roof-${s}`, GABLE_ROOF, [1.28 * sign, 0.8, 0.58], [0.9, 0.38, 0.9], roof),
      m.box(
        `booth-window-${s}`,
        [1.28 * sign - 0.36 * sign, 0.5, 0.58],
        [0.03, 0.26, 0.4],
        "#8fcbd9",
      ),
      m.box(`booth-counter-${s}`, [1.28 * sign - 0.4 * sign, 0.36, 0.58], [0.1, 0.04, 0.44], trim),
    )
  }
  nodes.push(
    m.box("lintel", [0, 1.68, 0], [1.66, 0.36, 0.2], "#2f6b58"),
    m.box("sign", [0, 1.68, 0.11], [1.2, 0.24, 0.03], "#e0bf65"),
    m.box("sign-back", [0, 1.68, -0.11], [1.2, 0.24, 0.03], "#e0bf65"),
    m.box("roof", [0, 1.92, 0], [1.8, 0.1, 0.36], roof),
  )
  // "ZOO" lettering as raised blocks on the outward face of the sign.
  const letter = (part: string, x: number, y: number, w: number, h: number, tilt = 0) =>
    m.box(
      `letter-${part}`,
      [x, 1.68 + y, 0.13],
      [w, h, 0.02],
      "#2f4a3d",
      tilt === 0 ? {} : { rotation: axisQuaternion("z", tilt) },
    )
  nodes.push(
    letter("z-top", -0.3, 0.07, 0.18, 0.035),
    letter("z-mid", -0.3, 0, 0.035, 0.2, -0.85),
    letter("z-bottom", -0.3, -0.07, 0.18, 0.035),
    ...[0, 0.3].flatMap((offset, index) => [
      letter(`o${index}-l`, offset - 0.07, 0, 0.035, 0.17),
      letter(`o${index}-r`, offset + 0.07, 0, 0.035, 0.17),
      letter(`o${index}-t`, offset, 0.07, 0.17, 0.035),
      letter(`o${index}-b`, offset, -0.07, 0.17, 0.035),
    ]),
  )
  return nodes
}

function depotNodes(snapshot: Snapshot): RendererSceneNode[] {
  const depot = snapshot.animal_care_depot
  const x = depot.x + 0.5
  const z = depot.y + 0.5
  const m = model("building:depot", { x, z })
  const nodes: RendererSceneNode[] = [
    blobShadow("building:depot:shadow", x, z, 0.62, 0.18),
    m.box("plinth", [0, 0.04, 0], [0.98, 0.08, 0.92], "#a9a68f"),
    m.box("shell", [0, 0.42, -0.02], [0.88, 0.7, 0.78], "#d1cbb2"),
    m.mesh("roof", GABLE_ROOF, [0, 0.77, -0.02], [1.0, 0.36, 0.92], "#56706c"),
    m.box("garage-door", [0, 0.32, 0.375], [0.5, 0.5, 0.03], "#42695f"),
    m.box("sign", [0, 0.68, 0.38], [0.5, 0.12, 0.03], "#e0bf65"),
    m.box("window", [0.33, 0.47, 0.375], [0.14, 0.14, 0.02], "#8fcbd9"),
    m.cylinder("vent", [-0.25, 1.05, -0.1], 0.05, 0.2, "#6f7e7b"),
  ]
  for (const [index, y] of [0.16, 0.26, 0.36, 0.46].entries()) {
    nodes.push(m.box(`slat:${index}`, [0, y, 0.393], [0.46, 0.018, 0.01], "#e8dfc2"))
  }
  // Feed crates stacked beside the depot reflect current stock.
  const crates = Math.min(depot.feed_crates, 6)
  for (let index = 0; index < crates; index += 1) {
    const column = index % 3
    const layer = Math.floor(index / 3)
    nodes.push(
      m.box(
        `crate:${index}`,
        [-0.34 + column * 0.12, 0.05 + layer * 0.1, 0.42],
        [0.1, 0.09, 0.1],
        "#b8874f",
      ),
    )
  }
  return nodes
}

function concessionNodes(snapshot: Snapshot): RendererSceneNode[] {
  const nodes: RendererSceneNode[] = []
  for (const stand of snapshot.concessions) {
    const x = stand.x + 0.5
    const z = stand.y + 0.5
    const yaw = facingNearestPath(snapshot, stand.x, stand.y)
    const m = model(`concession:${stand.id}`, { x, z }, yaw)
    const accent: Hex = stand.kind === "food" ? "#e0913f" : "#4fa9c7"
    let awning: Hex = accent
    if (stand.service_state === "failed") {
      awning = "#7c4a44"
    } else if (stand.service_state === "degraded") {
      awning = shade(accent, -0.3)
    }

    nodes.push(
      blobShadow(`concession:${stand.id}:shadow`, x, z, 0.5, 0.16),
      m.box("booth", [0, 0.34, -0.06], [0.72, 0.62, 0.56], "#f2ead2"),
      m.box("service-window", [0, 0.44, 0.225], [0.5, 0.24, 0.02], "#4a3a26"),
      m.box("counter", [0, 0.3, 0.29], [0.64, 0.06, 0.14], "#caa06a"),
      m.cylinder("post-left", [-0.36, 0.52, 0.34], 0.02, 1.0, "#f2ead2"),
      m.cylinder("post-right", [0.36, 0.52, 0.34], 0.02, 1.0, "#f2ead2"),
      m.box("sign", [0, 0.86, -0.06], [0.46, 0.2, 0.06], awning),
    )
    for (const [index, stripeX] of [-0.32, -0.16, 0, 0.16, 0.32].entries()) {
      nodes.push(
        m.box(
          `awning:${index}`,
          [stripeX, 0.72, 0.18],
          [0.16, 0.05, 0.5],
          index % 2 ? "#fbf6ea" : awning,
          {
            rotation: axisQuaternion("x", 0.22),
          },
        ),
      )
    }
    if (stand.kind === "food") {
      nodes.push(
        m.cylinder("bun-bottom", [0, 1.0, -0.06], 0.1, 0.04, "#d99b4a"),
        m.cylinder("patty", [0, 1.045, -0.06], 0.095, 0.035, "#6b4b33"),
        m.sphere("bun-top", [0, 1.07, -0.06], 0.1, "#e0a653", { scale: [1, 0.5, 1] }),
      )
    } else {
      nodes.push(
        m.cylinder("cup", [0, 1.05, -0.06], 0.07, 0.17, "#fbf6ea"),
        m.cylinder("straw", [0.03, 1.18, -0.06], 0.012, 0.14, "#c9483b"),
      )
    }
    if (stand.service_state === "failed") {
      nodes.push(m.box("closed-board", [0, 0.44, 0.245], [0.52, 0.2, 0.02], "#6b4e32"))
    }
  }
  return nodes
}

function facingNearestPath(snapshot: Snapshot, x: number, y: number) {
  const candidates: [FenceSide, number, number][] = [
    ["south", x, y + 1],
    ["east", x + 1, y],
    ["north", x, y - 1],
    ["west", x - 1, y],
  ]
  for (const [side, tx, ty] of candidates) {
    const tile = tileAt(snapshot, tx, ty)
    if (tile && (tile.kind === "path" || tile.kind === "entrance")) {
      return yawFacing(side)
    }
  }
  return 0
}

function tileAt(snapshot: Snapshot, x: number, y: number): Tile | undefined {
  if (x < 0 || y < 0 || x >= snapshot.width || y >= snapshot.height) {
    return undefined
  }
  const tile = snapshot.tiles[y * snapshot.width + x]
  if (tile && tile.x === x && tile.y === y) {
    return tile
  }
  return snapshot.tiles.find((candidate) => candidate.x === x && candidate.y === y)
}

// ---------------------------------------------------------------------------------------------
// Habitat furnishing

function habitatNodes(habitat: Habitat, selected: boolean): RendererSceneNode[] {
  const nodes: RendererSceneNode[] = []
  const id = `habitat:${habitat.id}`
  const right = habitat.x + habitat.width
  const bottom = habitat.y + habitat.height

  // Water trough against the north fence.
  const trough = model(`${id}:trough`, { x: habitat.x + 0.55, z: habitat.y + 0.3 })
  nodes.push(
    trough.box("basin", [0, 0.08, 0], [0.5, 0.14, 0.22], "#7d6a52"),
    trough.box(
      "water",
      [0, 0.155, 0],
      [0.42, 0.02, 0.15],
      habitat.water > 30 ? "#5fb3d1" : "#8e8a6d",
    ),
  )

  if (
    habitat.species === "flamingo" ||
    habitat.species === "capybara" ||
    habitat.species === "penguin"
  ) {
    const pond = model(`${id}:pond`, {
      x: habitat.x + habitat.width * 0.62,
      z: habitat.y + habitat.height * 0.6,
    })
    const radius = Math.min(habitat.width, habitat.height) * 0.28
    nodes.push(
      pond.cylinder("rim", [0, 0.012, 0], radius + 0.08, 0.02, "#b9a987"),
      pond.cylinder(
        "water",
        [0, 0.02, 0],
        radius,
        0.02,
        habitat.species === "penguin" ? "#6fb6d6" : "#5aa7c4",
      ),
    )
  }

  // A couple of rocks per habitat for texture.
  for (let index = 0; index < 2; index += 1) {
    const rx = habitat.x + 0.4 + hash(habitat.id, index, 11) * (habitat.width - 0.8)
    const rz = habitat.y + 0.4 + hash(habitat.id, index, 12) * (habitat.height - 0.8)
    nodes.push(rock(`${id}:rock:${index}`, rx, rz, 0.8, hash(habitat.id, index)))
  }

  if (
    habitat.species === "giraffe" ||
    habitat.species === "zebra" ||
    habitat.species === "elephant"
  ) {
    const tx = right - 0.6
    const tz = bottom - 0.6
    const acacia = model(`${id}:acacia`, { x: tx, z: tz })
    nodes.push(
      blobShadow(`${id}:acacia:shadow`, tx, tz, 0.5),
      acacia.cylinder("trunk", [0, 0.5, 0], 0.06, 1.0, "#7a5536"),
      acacia.sphere("crown", [0, 1.08, 0], 0.55, "#6d9a3f", { scale: [1, 0.32, 1] }),
    )
  }

  if (habitat.has_shelter) {
    const shelter = model(`${id}:shelter`, { x: right - 0.55, z: habitat.y + 0.55 }, Math.PI)
    nodes.push(
      shelter.box("back", [0, 0.3, 0.18], [0.72, 0.6, 0.08], "#8d6a45"),
      shelter.box("left", [-0.32, 0.3, 0], [0.08, 0.6, 0.44], "#8d6a45"),
      shelter.box("right", [0.32, 0.3, 0], [0.08, 0.6, 0.44], "#8d6a45"),
      shelter.mesh("roof", GABLE_ROOF, [0, 0.6, 0], [0.86, 0.26, 0.6], "#6c7a4a"),
    )
  }

  if (habitat.animals === 0) {
    // Signpost marking an empty enclosure waiting for animals.
    const sign = model(`${id}:vacant`, {
      x: habitat.x + habitat.width / 2,
      z: habitat.y + habitat.height / 2,
    })
    nodes.push(
      sign.cylinder("post", [0, 0.3, 0], 0.03, 0.6, "#7a5536"),
      sign.box("board", [0, 0.58, 0], [0.42, 0.24, 0.04], "#f2d274"),
      sign.box("plus-h", [0, 0.58, 0.025], [0.2, 0.05, 0.01], "#2f6b58"),
      sign.box("plus-v", [0, 0.58, 0.025], [0.05, 0.2, 0.01], "#2f6b58"),
    )
  }

  if (selected) {
    const x = habitat.x + habitat.width / 2
    const z = habitat.y + habitat.height / 2
    nodes.push(
      node(
        `${id}:selection`,
        [x, 0.03, z],
        boxGeometry([habitat.width - 0.1, 0.02, habitat.height - 0.1]),
        "#f6d36f",
        {
          opacity: 0.28,
        },
      ),
    )
  }
  return nodes
}

// ---------------------------------------------------------------------------------------------
// Animals — low-poly figures built facing local +z.

type Figure = ReturnType<typeof model>

function legs(
  m: Figure,
  spreadX: number,
  spreadZ: number,
  height: number,
  radius: number,
  color: Hex,
) {
  const feet: [number, number][] = [
    [-spreadX, -spreadZ],
    [spreadX, -spreadZ],
    [-spreadX, spreadZ],
    [spreadX, spreadZ],
  ]
  return feet.map(([lx, lz], index) =>
    m.cylinder(`leg:${index}`, [lx, height / 2, lz], radius, height, color),
  )
}

const ANIMAL_SHADOW_SIZE: Partial<Record<SpeciesKey, number>> = { elephant: 0.4, giraffe: 0.3 }
const ANIMAL_PICK_HEIGHT: Partial<Record<SpeciesKey, number>> = { giraffe: 0.7, elephant: 0.45 }

function animalFigure(m: Figure, species: SpeciesKey, stride: number): RendererSceneNode[] {
  switch (species) {
    case "capybara":
      return [
        ...legs(m, 0.07, 0.09, 0.08, 0.025, "#6b4a30"),
        m.sphere("body", [0, 0.15, 0], 0.13, "#9a6b43", { scale: [0.85, 0.75, 1.3] }),
        m.box("head", [0, 0.19, 0.17], [0.12, 0.12, 0.14], "#8f623c"),
        m.box("snout", [0, 0.17, 0.25], [0.1, 0.08, 0.05], "#5b3d26"),
      ]
    case "flamingo":
      return [
        m.cylinder("leg:0", [-0.025, 0.14, 0], 0.01, 0.28, "#e07a8e"),
        m.cylinder("leg:1", [0.025, 0.14 + stride * 0.02, 0], 0.01, 0.28, "#e07a8e"),
        m.sphere("body", [0, 0.33, -0.02], 0.09, "#f29bb0", { scale: [0.8, 0.75, 1.3] }),
        m.cylinder("neck", [0, 0.45, 0.07], 0.015, 0.22, "#f29bb0", {
          rotation: axisQuaternion("x", 0.35),
        }),
        m.sphere("head", [0, 0.56, 0.11], 0.035, "#f5a8bb"),
        m.box("beak", [0, 0.55, 0.15], [0.02, 0.02, 0.05], "#2c2c2c"),
      ]
    case "zebra":
      return [
        ...legs(m, 0.07, 0.13, 0.22, 0.025, "#f4f4f0"),
        m.box("body", [0, 0.3, 0], [0.18, 0.17, 0.38], "#f4f4f0"),
        ...[-0.12, -0.04, 0.04, 0.12].map((sz, i) =>
          m.box(`stripe:${i}`, [0, 0.3, sz], [0.185, 0.175, 0.03], "#262626"),
        ),
        m.box("neck", [0, 0.4, 0.2], [0.08, 0.18, 0.08], "#f4f4f0", {
          rotation: axisQuaternion("x", 0.5),
        }),
        m.box("head", [0, 0.47, 0.28], [0.08, 0.08, 0.16], "#e9e9e4"),
        m.box("mane", [0, 0.45, 0.18], [0.03, 0.12, 0.12], "#262626", {
          rotation: axisQuaternion("x", 0.5),
        }),
      ]
    case "giraffe":
      return [
        ...legs(m, 0.07, 0.11, 0.42, 0.025, "#d9a64f"),
        m.box("body", [0, 0.5, 0], [0.18, 0.18, 0.34], "#e3b35a"),
        m.box("spot:0", [0.092, 0.52, 0.05], [0.01, 0.07, 0.07], "#9a6128"),
        m.box("spot:1", [-0.092, 0.48, -0.07], [0.01, 0.07, 0.07], "#9a6128"),
        m.cylinder("neck", [0, 0.8, 0.17], 0.035, 0.52, "#e3b35a", {
          rotation: axisQuaternion("x", 0.3),
        }),
        m.box("head", [0, 1.06, 0.26], [0.07, 0.07, 0.15], "#e3b35a"),
        m.cylinder("ossicone", [0, 1.12, 0.22], 0.01, 0.06, "#6b4a30"),
      ]
    case "elephant":
      return [
        ...legs(m, 0.12, 0.14, 0.24, 0.055, "#8c8f93"),
        m.sphere("body", [0, 0.42, 0], 0.24, "#9a9da1", { scale: [0.9, 0.85, 1.2] }),
        m.sphere("head", [0, 0.5, 0.3], 0.14, "#9a9da1"),
        m.box("ear-left", [-0.14, 0.52, 0.26], [0.03, 0.2, 0.16], "#8a8d91"),
        m.box("ear-right", [0.14, 0.52, 0.26], [0.03, 0.2, 0.16], "#8a8d91"),
        m.cylinder("trunk", [0, 0.3, 0.43], 0.035, 0.3, "#8c8f93", {
          rotation: axisQuaternion("x", 0.25 + stride * 0.1),
        }),
      ]
    case "penguin":
      return [
        m.sphere("body", [0, 0.16, 0], 0.09, "#26282c", { scale: [0.9, 1.6, 0.85] }),
        m.sphere("belly", [0, 0.15, 0.035], 0.07, "#f4f4f0", { scale: [0.85, 1.5, 0.7] }),
        m.sphere("head", [0, 0.31, 0.01], 0.055, "#26282c"),
        m.box("beak", [0, 0.31, 0.07], [0.025, 0.02, 0.04], "#f0a53a"),
        m.box("feet", [0, 0.01, 0.03], [0.1, 0.02, 0.06], "#f0a53a"),
      ]
    default:
      throw new Error(`Unsupported animal presentation: ${species}`)
  }
}

function animalNodes(animals: Animal[], input: SceneFrameInput) {
  const nodes: RendererSceneNode[] = []
  const anchors: PickAnchor[] = []
  for (const animal of animals) {
    const slotX = ((animal.slot % 3) - 1) * 0.26
    const slotZ = ((Math.floor(animal.slot / 3) % 2) - 0.5) * 0.3
    const [x, z] = input.resolveActor(
      `animal:${animal.id}`,
      animal.x + 0.5 + slotX,
      animal.y + 0.5 + slotZ,
    )
    const phase = animal.animation_phase * 0.37 + animal.id
    const wander = input.animate ? Math.sin(input.timeSeconds * 0.35 + phase) * 0.8 : 0
    const stride = input.animate ? Math.sin(input.timeSeconds * 4 + phase) : 0
    const yaw = phase + wander
    const m = model(`animal:${animal.id}`, { x, z }, yaw)
    const size = ANIMAL_SHADOW_SIZE[animal.species] ?? 0.22
    nodes.push(
      blobShadow(`animal:${animal.id}:shadow`, x, z, size),
      ...animalFigure(m, animal.species, stride),
    )
    const height = ANIMAL_PICK_HEIGHT[animal.species] ?? 0.25
    anchors.push({
      kind: "animal",
      habitatId: animal.habitat_id,
      point: [x, height, z],
      radius: size + 0.1,
    })
  }
  return { nodes, anchors }
}

// ---------------------------------------------------------------------------------------------
// People

const SHIRTS: Hex[] = [
  "#e0604f",
  "#4d8fd1",
  "#f2c14e",
  "#7cc3a5",
  "#b77fd1",
  "#f08a3e",
  "#5bb7d6",
  "#e67fa2",
]
const SKIN: Hex[] = ["#f1c9a5", "#d9a47a", "#b07a52", "#7a5237", "#e8b894"]

function person(
  id: string,
  x: number,
  z: number,
  yaw: number,
  bob: number,
  colors: { shirt: Hex; skin: Hex; legs: Hex; hat?: Hex },
) {
  const m = model(id, { x, z }, yaw, bob)
  const nodes = [
    blobShadow(`${id}:shadow`, x, z, 0.1, 0.25),
    m.cylinder("legs", [0, 0.09, 0], 0.045, 0.18, colors.legs),
    m.cylinder("torso", [0, 0.25, 0], 0.06, 0.16, colors.shirt),
    m.sphere("head", [0, 0.39, 0], 0.055, colors.skin),
  ]
  if (colors.hat) {
    nodes.push(m.cylinder("hat", [0, 0.445, 0], 0.058, 0.03, colors.hat))
  }
  return nodes
}

/** Stable per-id offset inside a tile so co-located people do not overlap. */
function jitter(id: number, salt: number) {
  return (hash(id, salt) - 0.5) * 0.56
}

function guestNodes(guests: Guest[], input: SceneFrameInput) {
  const nodes: RendererSceneNode[] = []
  const anchors: PickAnchor[] = []
  for (const guest of guests) {
    const key = `guest:${guest.id}`
    const [x, z] = input.resolveActor(
      key,
      guest.x + 0.5 + jitter(guest.id, 1),
      guest.y + 0.5 + jitter(guest.id, 2),
    )
    const walking = guest.state !== "viewing"
    const bob =
      input.animate && walking ? Math.abs(Math.sin(input.timeSeconds * 9 + guest.id)) * 0.03 : 0
    const yaw = hash(guest.id, Math.floor(input.timeSeconds / 3)) * Math.PI * 2
    nodes.push(
      ...person(key, x, z, yaw, bob, {
        shirt: SHIRTS[guest.id % SHIRTS.length] ?? "#ffffff",
        skin: SKIN[(guest.id * 7) % SKIN.length] ?? "#f1c9a5",
        legs: "#3d4b63",
      }),
    )
    if (guest.id === input.overlay.selectedGuestId) {
      nodes.push(
        node(`${key}:ring`, [x, 0.02, z], cylinderGeometry(0.2, 0.015), "#f6d36f", {
          opacity: 0.8,
        }),
        node(
          `${key}:marker`,
          [x, 0.68 + Math.sin(input.timeSeconds * 3) * 0.04, z],
          sphereGeometry(0.06),
          "#f6d36f",
        ),
      )
    }
    anchors.push({ kind: "guest", id: guest.id, point: [x, 0.25, z], radius: 0.22 })
  }
  return { nodes, anchors }
}

function staffNodes(snapshot: Snapshot, input: SceneFrameInput): RendererSceneNode[] {
  const nodes: RendererSceneNode[] = []
  for (const janitor of snapshot.animal_care_depot.janitors) {
    const key = `janitor:${janitor.id}`
    const [x, z] = input.resolveActor(
      key,
      janitor.x + 0.5 + jitter(janitor.id, 3) * 0.5,
      janitor.y + 0.5 + jitter(janitor.id, 4) * 0.5,
    )
    const yaw = hash(janitor.id, 9) * Math.PI * 2
    nodes.push(
      ...person(key, x, z, yaw, 0, {
        shirt: "#3f8f5f",
        skin: SKIN[janitor.id % SKIN.length] ?? "#f1c9a5",
        legs: "#2e5e43",
        hat: "#2e5e43",
      }),
      model(key, { x, z }, yaw).cylinder("broom", [0.09, 0.2, 0.04], 0.01, 0.4, "#a07b4f", {
        rotation: axisQuaternion("z", 0.3),
      }),
    )
  }
  for (const mechanic of snapshot.animal_care_depot.mechanics) {
    const key = `mechanic:${mechanic.id}`
    const [x, z] = input.resolveActor(
      key,
      mechanic.x + 0.5 + jitter(mechanic.id, 5) * 0.5,
      mechanic.y + 0.5 + jitter(mechanic.id, 6) * 0.5,
    )
    const yaw = hash(mechanic.id, 10) * Math.PI * 2
    nodes.push(
      ...person(key, x, z, yaw, 0, {
        shirt: "#e0873a",
        skin: SKIN[(mechanic.id + 2) % SKIN.length] ?? "#f1c9a5",
        legs: "#40506a",
        hat: "#f2c14e",
      }),
      model(key, { x, z }, yaw).box("toolbox", [0.1, 0.08, 0], [0.08, 0.06, 0.05], "#c9483b"),
    )
  }
  for (const keeper of snapshot.animal_care_depot.keepers) {
    // Keepers at the depot are inside the building; walking and working keepers are shown
    // exactly where zoo-core puts them.
    if (keeper.activity === "at_depot") {
      continue
    }
    const key = `keeper:${keeper.id}`
    const [x, z] = input.resolveActor(key, keeper.x + 0.5, keeper.y + 0.5)
    const yaw = hash(keeper.id, 11) * Math.PI * 2
    const bob =
      input.animate && keeper.activity !== "working"
        ? Math.abs(Math.sin(input.timeSeconds * 9 + keeper.id)) * 0.03
        : 0
    nodes.push(
      ...person(key, x, z, yaw, bob, {
        shirt: "#c99a3c",
        skin: SKIN[(keeper.id + 4) % SKIN.length] ?? "#f1c9a5",
        legs: "#5b4a2f",
        hat: "#8a6d3b",
      }),
      node(`${key}:marker`, [x, 0.62, z], sphereGeometry(0.05), "#ffb347"),
    )
    if (keeper.assigned_habitat_id === input.overlay.selectedHabitatId) {
      for (const [index, step] of keeper.route.slice(1).entries()) {
        nodes.push(
          node(
            `${key}:route:${index}`,
            [step.x + 0.5, 0.06, step.y + 0.5],
            cylinderGeometry(0.06, 0.02),
            "#ffb347",
            { opacity: 0.7 },
          ),
        )
      }
    }
  }
  return nodes
}

function litterNodes(snapshot: Snapshot): RendererSceneNode[] {
  return snapshot.litter.flatMap((task) => {
    const x = task.x + 0.5 + jitter(task.id, 7) * 0.6
    const z = task.y + 0.5 + jitter(task.id, 8) * 0.6
    const m = model(`litter:${task.id}`, { x, z }, hash(task.id) * 6)
    return [
      m.box("paper", [0, 0.035, 0], [0.09, 0.02, 0.07], "#f4f1e6"),
      m.cylinder(
        "can",
        [0.07, 0.04, 0.03],
        0.02,
        0.05,
        task.assigned_janitor_id === null ? "#d0584a" : "#e0bf65",
      ),
    ]
  })
}

function maintenanceNodes(snapshot: Snapshot, input: SceneFrameInput): RendererSceneNode[] {
  return snapshot.maintenance.map((task) => {
    const bob = input.animate ? Math.sin(input.timeSeconds * 3 + task.id) * 0.06 : 0
    const spin = input.timeSeconds * 1.5
    return node(
      `maintenance:${task.id}`,
      [task.x + 0.5, 1.45 + bob, task.y + 0.5],
      boxGeometry([0.16, 0.16, 0.16]),
      task.assigned_mechanic_id === null ? "#e0473a" : "#f0a53a",
      { rotation: multiplyQuaternion(yawQuaternion(spin), axisQuaternion("x", Math.PI / 4)) },
    )
  })
}

// ---------------------------------------------------------------------------------------------
// Interaction overlays (hover, habitat drag ghost)

function toolColor(tool: Tool): Hex {
  switch (tool) {
    case "bulldoze":
      return "#e0473a"
    case "path":
      return "#f3e3b8"
    case "habitat":
    case "gate":
      return "#f6d36f"
    case "zone":
      return "#9fd0f6"
    case "select":
      return "#ffffff"
    default:
      return "#f6d36f"
  }
}

function overlayNodes(overlay: SceneOverlay): RendererSceneNode[] {
  const nodes: RendererSceneNode[] = []
  if (overlay.ghostTiles.length > 0) {
    const color: Hex = overlay.placement?.ok ? "#9fe07a" : "#e0604f"
    for (const tile of overlay.ghostTiles) {
      nodes.push(
        node(
          `overlay:ghost:${tile.x}:${tile.y}`,
          [tile.x + 0.5, 0.045, tile.y + 0.5],
          boxGeometry([0.96, 0.02, 0.96]),
          color,
          {
            opacity: 0.42,
          },
        ),
      )
    }
  } else if (overlay.hoveredTile) {
    const { x, y } = overlay.hoveredTile
    nodes.push(
      node(
        "overlay:hover",
        [x + 0.5, 0.05, y + 0.5],
        boxGeometry([0.98, 0.02, 0.98]),
        toolColor(overlay.tool),
        {
          opacity: overlay.tool === "select" ? 0.22 : 0.4,
        },
      ),
    )
  }
  return nodes
}

// ---------------------------------------------------------------------------------------------

/** Identity of everything outside the park that scenery depends on: park size and gate. */
export function sceneryKey(snapshot: Snapshot): string {
  return `${snapshot.width}x${snapshot.height}@${snapshot.entrance.x},${snapshot.entrance.y}`
}

/** Trees, bushes, and rocks around the park; they change only when `sceneryKey` changes. */
export function buildSceneryNodes(snapshot: Snapshot): RendererSceneNode[] {
  return sceneryNodes(snapshot)
}

/**
 * Nodes that depend only on the snapshot and interaction overlay (scenery excluded). Callers cache this per
 * snapshot/overlay identity so per-frame actor animation does not rebuild terrain and scenery.
 */
export function buildStaticNodes(snapshot: Snapshot, overlay: SceneOverlay): RendererSceneNode[] {
  snapshot = presentationSnapshot(snapshot)
  const depot = snapshot.animal_care_depot
  const nodes = [
    ...terrainNodes(snapshot),
    ...snapshot.habitats.flatMap((habitat) =>
      habitatNodes(habitat, habitat.id === overlay.selectedHabitatId),
    ),
    ...fenceNodes(snapshot, overlay.placement),
    ...zoneNodes(snapshot, overlay),
    ...gateNodes(snapshot, overlay),
    ...entranceGateNodes(snapshot),
    ...depotNodes(snapshot),
    ...concessionNodes(snapshot),
    ...litterNodes(snapshot),
    ...overlayNodes(overlay),
  ]
  if (overlay.selectedDepot) {
    nodes.push(
      node(
        "building:depot:selection",
        [depot.x + 0.5, 0.03, depot.y + 0.5],
        boxGeometry([1, 0.02, 1]),
        "#f6d36f",
        {
          opacity: 0.35,
        },
      ),
    )
  }
  return nodes
}

/** Animated actors and markers, rebuilt every rendered frame, plus screen-pick anchors. */
export function buildActorFrame(input: SceneFrameInput): SceneFrame {
  const snapshot = presentationSnapshot(input.snapshot)
  const animals = animalNodes(snapshot.animals, input)
  const guests = guestNodes(snapshot.guests, input)
  const depot = snapshot.animal_care_depot

  const nodes = [
    ...animals.nodes,
    ...guests.nodes,
    ...staffNodes(snapshot, input),
    ...maintenanceNodes(snapshot, input),
  ]

  const anchors: PickAnchor[] = [
    ...guests.anchors,
    ...animals.anchors,
    ...snapshot.concessions.map(
      (stand) =>
        ({
          kind: "concession",
          id: stand.id,
          tile: { x: stand.x, y: stand.y },
          point: [stand.x + 0.5, 0.5, stand.y + 0.5],
          radius: 0.5,
        }) satisfies PickAnchor,
    ),
    { kind: "depot", point: [depot.x + 0.5, 0.5, depot.y + 0.5], radius: 0.55 },
  ]
  return { nodes, anchors }
}

/** Resolve validated content IDs to the renderer's existing presentation profiles.
 * This adapter never changes simulation records or selects gameplay rules.
 */
function presentationSnapshot(snapshot: Snapshot): Snapshot {
  const speciesAsset = (key: string | null) => {
    if (key === null) {
      return null
    }
    const offer = snapshot.species_catalog.find((entry) => entry.key === key)
    if (!offer) {
      throw new Error(`species/${key}/asset_ref: absent validated presentation`)
    }
    return offer.asset_ref
  }
  return {
    ...snapshot,
    habitats: snapshot.habitats.map((habitat) => ({
      ...habitat,
      species: speciesAsset(habitat.species),
    })),
    animals: snapshot.animals.map((animal) => ({
      ...animal,
      species: speciesAsset(animal.species) ?? animal.species,
    })),
    concessions: snapshot.concessions.map((stand) => {
      const offer = snapshot.facility_catalog.find((entry) => entry.key === stand.kind)
      if (!offer) {
        throw new Error(`facilities/${stand.kind}/asset_ref: absent validated presentation`)
      }
      return { ...stand, kind: offer.service }
    }),
  }
}
