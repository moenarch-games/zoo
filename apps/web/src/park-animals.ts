import type { ImportedCharacterInstance } from "@moritzbrantner/three-d-renderer/imported-character"

import type { AnimalCharacter, AnimalKit } from "./animal-kit"
import { ANIMAL_SCALE, type AnimalClip, type AnimalPose } from "./park-scene"

// Presents zoo-core's animals as `zoo-animals` character instances (#112). One instance per
// animal ID lives as long as the animal is in the snapshot. Each frame places it where the scene
// composition put the animal, turns it towards its travel and plays the clip zoo-core's state
// selects through the shared Rust pose runtime. Everything here is cosmetic and per mount: clip
// timing, LOD and reduced motion never reach zoo-core.

/** Cross-fade between clips; hides standing up and lying down (asset-tooling docs/zoo-animals.md). */
const CROSS_FADE_SECONDS = 0.4
/** Full-size walk pace the clips are authored for, in metres per second. */
const AUTHORED_WALK_PACE = 0.6
const WALK_RATE_LIMITS = [0.75, 2.5] as const
const TURN_PER_SECOND = 8
/** Below this projected height an animal is far away and re-poses at FAR_POSE_HZ only. */
export const FAR_PIXEL_HEIGHT = 14
const FAR_POSE_HZ = 6

/** The model's authored forward axis is +x; park yaw 0 faces +z. */
const MODEL_FORWARD_YAW = -Math.PI / 2

type Presented = {
  instance: ImportedCharacterInstance
  species: string
  clip: AnimalClip
  yaw: number
  x: number
  z: number
  /** Accumulated, not yet applied pose time (far LOD). */
  pending: number
  lod: "near" | "far" | "hidden" | "still"
}

type MovableRoot = {
  matrix: { fromArray(values: readonly number[]): unknown }
  matrixWorldNeedsUpdate: boolean
}

/** How one animal's projected size decides its pose detail this frame. */
export type AnimalVisibility = (pose: AnimalPose) => { visible: boolean; pixelHeight: number }

export type ParkAnimalsState = {
  instances: number
  /** Animal ID → clip, the zoo-core-selected presentation cue. */
  clips: Record<number, AnimalClip>
  lod: { near: number; far: number; hidden: number; still: number }
  created: number
  disposed: number
}

function modelMatrix(pose: AnimalPose, yaw: number) {
  const angle = yaw + MODEL_FORWARD_YAW
  const c = Math.cos(angle) * ANIMAL_SCALE
  const s = Math.sin(angle) * ANIMAL_SCALE
  // Column-major: uniform scale, rotation about +y, then the park position.
  return [c, 0, -s, 0, 0, ANIMAL_SCALE, 0, 0, s, 0, c, 0, pose.x, pose.lift, pose.z, 1]
}

function turnTowards(from: number, to: number, maxStep: number) {
  const delta = Math.atan2(Math.sin(to - from), Math.cos(to - from))
  return from + Math.max(-maxStep, Math.min(maxStep, delta))
}

export class ParkAnimals {
  private readonly animals = new Map<number, Presented>()
  private instancesChanged = true
  private created = 0
  private disposed = 0

  constructor(private readonly kit: AnimalKit) {}

  private character(species: string): AnimalCharacter {
    const character = this.kit.characters.get(species)
    if (!character) {
      throw new Error(`zoo-animals has no character for the ${species} presentation`)
    }
    return character
  }

  /**
   * Brings the instances in line with this frame's poses and advances their clips by `dt`
   * (zero while paused). `still` holds each clip's first frame (reduced motion). Returns whether
   * the instance set changed, which is when the renderer must be handed the new roots.
   */
  update(
    poses: readonly AnimalPose[],
    dt: number,
    options: { still: boolean; visibility: AnimalVisibility },
  ) {
    const seen = new Set<number>()
    for (const pose of poses) {
      seen.add(pose.id)
      let animal = this.animals.get(pose.id)
      if (animal && animal.species !== pose.species) {
        this.remove(pose.id)
        animal = undefined
      }
      const character = this.character(pose.species)
      if (!animal) {
        const yaw = pose.heading ?? (pose.seed % 628) / 100
        const instance = character.asset.createInstance({ modelMatrix: modelMatrix(pose, yaw) })
        const duration = character.clips.get(pose.clip) ?? 1
        instance.seek(pose.clip, options.still ? 0 : ((pose.seed % 997) / 997) * duration)
        animal = {
          instance,
          species: pose.species,
          clip: pose.clip,
          yaw,
          x: pose.x,
          z: pose.z,
          pending: 0,
          lod: "near",
        }
        this.animals.set(pose.id, animal)
        this.instancesChanged = true
        this.created += 1
      }
      this.place(animal, pose, dt)
      this.pose(animal, pose, character, dt, options)
    }
    for (const id of this.animals.keys()) {
      if (!seen.has(id)) {
        this.remove(id)
      }
    }
    const changed = this.instancesChanged
    this.instancesChanged = false
    return changed
  }

  private place(animal: Presented, pose: AnimalPose, dt: number) {
    if (pose.heading !== null) {
      animal.yaw = turnTowards(animal.yaw, pose.heading, TURN_PER_SECOND * dt)
    }
    const root = animal.instance.root as unknown as MovableRoot
    root.matrix.fromArray(modelMatrix(pose, animal.yaw))
    root.matrixWorldNeedsUpdate = true
  }

  private pose(
    animal: Presented,
    pose: AnimalPose,
    character: AnimalCharacter,
    dt: number,
    { still, visibility }: { still: boolean; visibility: AnimalVisibility },
  ) {
    const speed = dt > 0 ? Math.hypot(pose.x - animal.x, pose.z - animal.z) / dt : 0
    animal.x = pose.x
    animal.z = pose.z
    if (still) {
      if (animal.lod !== "still" || animal.clip !== pose.clip) {
        animal.clip = pose.clip
        animal.instance.seek(pose.clip, 0)
      }
      animal.lod = "still"
      return
    }
    if (animal.clip !== pose.clip) {
      animal.clip = pose.clip
      animal.instance.transition(pose.clip, CROSS_FADE_SECONDS)
    }
    // Walk cadence follows the presented ground speed at park scale.
    const rate =
      pose.clip === "walk"
        ? Math.min(
            WALK_RATE_LIMITS[1],
            Math.max(WALK_RATE_LIMITS[0], speed / (AUTHORED_WALK_PACE * ANIMAL_SCALE)),
          )
        : 1
    animal.pending += dt * rate
    const view = visibility(pose)
    if (!view.visible) {
      animal.lod = "hidden"
    } else {
      animal.lod = view.pixelHeight < FAR_PIXEL_HEIGHT ? "far" : "near"
    }
    // Off-screen animals keep their clip time but are not re-posed; far ones re-pose at a low rate.
    if (animal.lod === "hidden" || (animal.lod === "far" && animal.pending < 1 / FAR_POSE_HZ)) {
      const duration = character.clips.get(animal.clip) ?? 1
      animal.pending %= duration * 4
      return
    }
    animal.instance.advance(animal.pending)
    animal.pending = 0
  }

  private remove(id: number) {
    const animal = this.animals.get(id)
    if (animal) {
      animal.instance.dispose()
      this.animals.delete(id)
      this.instancesChanged = true
      this.disposed += 1
    }
  }

  instances(): ImportedCharacterInstance[] {
    return [...this.animals.values()].map((animal) => animal.instance)
  }

  get state(): ParkAnimalsState {
    const lod = { near: 0, far: 0, hidden: 0, still: 0 }
    const clips: Record<number, AnimalClip> = {}
    for (const [id, animal] of this.animals) {
      lod[animal.lod] += 1
      clips[id] = animal.clip
    }
    return {
      instances: this.animals.size,
      clips,
      lod,
      created: this.created,
      disposed: this.disposed,
    }
  }

  /** Disposes every instance, then the mount's character assets. */
  dispose() {
    // Deleting the visited entry while iterating a Map is safe.
    for (const id of this.animals.keys()) {
      this.remove(id)
    }
    this.kit.dispose()
  }
}
