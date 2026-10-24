import type {
  RendererEffectAtlas,
  RendererEffectBurst,
  RendererEffects,
} from "@moritzbrantner/three-d-renderer"

import type { EffectPiece, EffectsKit } from "./effects-kit"
import type { EffectSources, Presence } from "./park-scene"

// Cosmetic particle cues for the park view (#86): dust puffs while a keeper works in a habitat,
// a leaf flutter when a keeper or guest passes leafy scenery, and a splash when an animal steps
// into a habitat pond. Bursts are played by the shared renderer's `frame.effects.bursts` from the
// asset-tooling `zoo-effects` presets. Everything here is presentation: cues read presented actor
// positions, the clock is the view's cosmetic clock in seconds, seeds are a cosmetic hash, and
// nothing feeds back into zoo-core or touches game RNG.

/** Seconds between dust puffs while a keeper keeps working. */
const DUST_PERIOD = 1.3
/** A passer within this many metres of foliage flutters its leaves. */
const LEAF_RADIUS = 0.75
/** The same foliage flutters at most this often. */
const LEAF_COOLDOWN = 2.5
/** Concurrent bursts per effect; with the presets below the total stays under 64 billboards. */
const CAPS: Record<EffectPiece, number> = { dust: 4, leaf: 3, splash: 2 }
/** Which preset of the release each cue uses. */
const PRESET: Record<EffectPiece, "standard" | "subtle"> = {
  dust: "standard",
  leaf: "subtle",
  splash: "subtle",
}
/** Billboard budget handed to the renderer (its own default cap is also 64). */
export const EFFECT_BUDGET = 64

type Burst = { piece: EffectPiece; end: number; burst: RendererEffectBurst }

/** 32-bit FNV-1a of a cue key; the cosmetic seed of a burst. */
function cueSeed(text: string) {
  let h = 2166136261
  for (let index = 0; index < text.length; index += 1) {
    h ^= text.charCodeAt(index)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

export type ParkEffectsState = {
  /** Live bursts per effect, for the debug hook and the browser proof. */
  live: Record<EffectPiece, number>
  /** Bursts started since mount, per effect. */
  started: Record<EffectPiece, number>
}

export class ParkEffects {
  private bursts: Burst[] = []
  private serial = 0
  private nextDust = new Map<string, number>()
  private near = new Map<string, Set<string>>()
  private lastLeaf = new Map<string, number>()
  private inPond = new Map<string, boolean>()
  readonly state: ParkEffectsState = {
    live: { dust: 0, leaf: 0, splash: 0 },
    started: { dust: 0, leaf: 0, splash: 0 },
  }

  constructor(private readonly kit: EffectsKit) {}

  /** True while any burst may still draw, so a paused view keeps rendering until they finish. */
  get active() {
    return this.bursts.length > 0
  }

  /**
   * The renderer `effects` for one frame. `emit: false` (paused) starts no new bursts but lets
   * live ones finish; `enabled: false` (reduced motion) draws nothing and releases the atlases.
   */
  frame(
    time: number,
    presence: readonly Presence[],
    sources: EffectSources,
    emit: boolean,
    enabled: boolean,
  ): RendererEffects {
    if (!enabled) {
      this.reset()
      return { time, enabled: false, atlases: [], instances: [] }
    }
    this.bursts = this.bursts.filter((entry) => entry.end > time)
    if (emit) {
      this.cue(time, presence, sources)
    }
    const used = new Set<EffectPiece>()
    for (const piece of Object.keys(this.state.live) as EffectPiece[]) {
      this.state.live[piece] = 0
    }
    for (const entry of this.bursts) {
      used.add(entry.piece)
      this.state.live[entry.piece] += 1
    }
    // Atlases are declared only while a burst uses them, so an idle view holds no effect textures.
    const atlases: RendererEffectAtlas[] = [...used].map((piece) => this.kit.atlases[piece])
    return {
      time,
      maxInstances: EFFECT_BUDGET,
      atlases,
      instances: [],
      bursts: this.bursts.map((entry) => entry.burst),
    }
  }

  private reset() {
    this.bursts = []
    this.nextDust.clear()
    this.near.clear()
    this.lastLeaf.clear()
    this.inPond.clear()
    for (const piece of Object.keys(this.state.live) as EffectPiece[]) {
      this.state.live[piece] = 0
    }
  }

  private start(piece: EffectPiece, cue: string, time: number, origin: [number, number, number]) {
    if (this.bursts.filter((entry) => entry.piece === piece).length >= CAPS[piece]) {
      return
    }
    const entry = this.kit.manifest.pieces[piece]
    const preset = entry.presets[PRESET[piece]]
    this.serial += 1
    const id = `${piece}:${cue}:${this.serial}`
    this.bursts.push({
      piece,
      end: time + (preset.emitDuration ?? 0) + preset.lifetime,
      burst: {
        ...preset,
        id,
        atlas: this.kit.atlases[piece].resourceKey,
        space: "world",
        origin,
        startTime: time,
        seed: cueSeed(id),
        scale: entry.scale,
      },
    })
    this.state.started[piece] += 1
  }

  private cue(time: number, presence: readonly Presence[], sources: EffectSources) {
    const seen = new Set<string>()
    for (const actor of presence) {
      seen.add(actor.key)
      // Dust: a keeper working in a habitat kicks up a puff every DUST_PERIOD seconds.
      if (actor.kind === "keeper") {
        if (actor.working) {
          const due = this.nextDust.get(actor.key) ?? time
          if (time >= due) {
            this.start("dust", actor.key, time, [actor.x, 0.12, actor.z])
            this.nextDust.set(actor.key, time + DUST_PERIOD)
          }
        } else {
          this.nextDust.delete(actor.key)
        }
      }
      // Leaves: a keeper or guest coming within reach of foliage.
      if (actor.kind !== "animal") {
        const previous = this.near.get(actor.key) ?? new Set<string>()
        const current = new Set<string>()
        for (const plant of sources.foliage) {
          if (Math.hypot(plant.x - actor.x, plant.z - actor.z) > LEAF_RADIUS) {
            continue
          }
          current.add(plant.id)
          const last = this.lastLeaf.get(plant.id) ?? -Infinity
          if (!previous.has(plant.id) && time - last >= LEAF_COOLDOWN) {
            this.lastLeaf.set(plant.id, time)
            this.start("leaf", plant.id, time, [plant.x, plant.height, plant.z])
          }
        }
        this.near.set(actor.key, current)
      }
      // Splash: an animal stepping into a habitat pond.
      if (actor.kind === "animal") {
        const pond = sources.ponds.find(
          (candidate) =>
            Math.hypot(candidate.x - actor.x, candidate.z - actor.z) < candidate.radius,
        )
        if (pond && !this.inPond.get(actor.key)) {
          this.start("splash", actor.key, time, [actor.x, 0.05, actor.z])
        }
        this.inPond.set(actor.key, Boolean(pond))
      }
    }
    for (const map of [this.nextDust, this.near, this.inPond]) {
      for (const key of map.keys()) {
        if (!seen.has(key)) {
          map.delete(key)
        }
      }
    }
  }
}
