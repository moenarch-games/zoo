import type { RendererEffectAtlas } from "@moritzbrantner/three-d-renderer"
import type { BurstPreset } from "@moritzbrantner/three-d-renderer/flipbook"

// The asset-tooling `zoo-effects` particle flipbooks staged by scripts/stage-asset-releases.ts
// (#86). Each atlas PNG is checked against its staged SHA-256 and decoded once per page into the
// RGBA bytes the shared renderer's `frame.effects` takes; each mount's renderer owns its textures.

export const EFFECT_PIECES = ["dust", "leaf", "splash"] as const
export type EffectPiece = (typeof EFFECT_PIECES)[number]

export type EffectsManifest = {
  schemaVersion: 1
  release: string
  tarSha256: string
  manifestSha256: string
  assetToolingCommit: string
  profile: string
  recipe: { source: string; sha256: string }
  provenance: string
  pieces: Record<
    EffectPiece,
    {
      key: string
      variant: string
      file: string
      sha256: string
      byteLength: number
      /** Recommended world quad size in metres. */
      scale: number
      atlas: {
        width: number
        height: number
        columns: number
        rows: number
        frameCount: number
        filter: "linear" | "nearest"
      }
      presets: { standard: BurstPreset; subtle: BurstPreset }
    }
  >
}

export type EffectsKit = {
  manifest: EffectsManifest
  atlases: Record<EffectPiece, RendererEffectAtlas>
}

const effectsUrl = (file: string) => new URL(`zoo-effects/${file}`, document.baseURI)

let pendingManifest: Promise<EffectsManifest> | null = null
let pendingKit: Promise<EffectsKit> | null = null

/** The staged effects manifest, fetched once per page. Rejects when the build staged none. */
export function loadEffectsManifest(): Promise<EffectsManifest> {
  pendingManifest ??= (async () => {
    const response = await fetch(effectsUrl("effects.json"))
    if (!response.ok) {
      throw new Error(`zoo-effects/effects.json: HTTP ${response.status}`)
    }
    const manifest = (await response.json()) as EffectsManifest
    if (manifest.schemaVersion !== 1 || EFFECT_PIECES.some((piece) => !manifest.pieces?.[piece])) {
      throw new Error("zoo-effects/effects.json does not list every effect")
    }
    return manifest
  })()
  pendingManifest.catch(() => {
    pendingManifest = null
  })
  return pendingManifest
}

const hex = (buffer: ArrayBuffer) =>
  [...new Uint8Array(buffer)].map((byte) => byte.toString(16).padStart(2, "0")).join("")

async function loadAtlas(manifest: EffectsManifest, piece: EffectPiece) {
  const entry = manifest.pieces[piece]
  const response = await fetch(effectsUrl(entry.file))
  if (!response.ok) {
    throw new Error(`zoo-effects/${entry.file}: HTTP ${response.status}`)
  }
  const bytes = await response.arrayBuffer()
  if (hex(await crypto.subtle.digest("SHA-256", bytes)) !== entry.sha256) {
    throw new Error(`zoo-effects/${entry.file} does not match its staged SHA-256`)
  }
  const { width, height, columns, rows, frameCount, filter } = entry.atlas
  const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }), {
    premultiplyAlpha: "none",
    colorSpaceConversion: "none",
  })
  if (image.width !== width || image.height !== height) {
    throw new Error(
      `zoo-effects/${entry.file} is ${image.width}×${image.height}, not ${width}×${height}`,
    )
  }
  const canvas = new OffscreenCanvas(width, height)
  const context = canvas.getContext("2d")!
  context.drawImage(image, 0, 0)
  image.close()
  const atlas: RendererEffectAtlas = {
    resourceKey: `zoo-effects:${entry.sha256}`,
    width,
    height,
    pixels: context.getImageData(0, 0, width, height).data,
    columns,
    rows,
    frameCount,
    filter,
  }
  return [piece, atlas] as const
}

/** Fetches, checks and decodes the effect atlases; rejects (and retries next call) on any mismatch. */
export function loadEffectsKit(): Promise<EffectsKit> {
  pendingKit ??= (async () => {
    const manifest = await loadEffectsManifest()
    const atlases = await Promise.all(EFFECT_PIECES.map((piece) => loadAtlas(manifest, piece)))
    return {
      manifest,
      atlases: Object.fromEntries(atlases) as Record<EffectPiece, RendererEffectAtlas>,
    }
  })()
  pendingKit.catch(() => {
    pendingKit = null
  })
  return pendingKit
}
