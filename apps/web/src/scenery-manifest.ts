// Provenance of the asset-tooling scenery staged by scripts/stage-asset-releases.ts (#59 S2).

export const SCENERY_PIECES = [
  "tree-round",
  "tree-pine",
  "tree-acacia",
  "bush-round",
  "grass-tuft",
  "rock-round",
  "rock-boulder",
] as const
export type SceneryPiece = (typeof SCENERY_PIECES)[number]

export type SceneryManifest = {
  schemaVersion: 1
  release: string
  tarSha256: string
  manifestSha256: string
  assetToolingCommit: string
  profile: string
  recipe: { script: string; sha256: string; blenderVersion: string }
  provenance: string
  pieces: Record<
    SceneryPiece,
    { key: string; variant: string; file: string; sha256: string; byteLength: number }
  >
}

export const sceneryUrl = (file: string) => new URL(`zoo-scenery/${file}`, document.baseURI)

let pending: Promise<SceneryManifest> | null = null

/** The staged scenery manifest, fetched once per page. Rejects when the build staged none. */
export function loadSceneryManifest(): Promise<SceneryManifest> {
  pending ??= (async () => {
    const response = await fetch(sceneryUrl("scenery.json"))
    if (!response.ok) {
      throw new Error(`zoo-scenery/scenery.json: HTTP ${response.status}`)
    }
    const manifest = (await response.json()) as SceneryManifest
    if (manifest.schemaVersion !== 1 || SCENERY_PIECES.some((piece) => !manifest.pieces?.[piece])) {
      throw new Error("zoo-scenery/scenery.json does not list every scenery piece")
    }
    return manifest
  })()
  pending.catch(() => {
    pending = null
  })
  return pending
}
