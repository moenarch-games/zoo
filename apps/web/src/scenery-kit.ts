import { adaptStaticGlb, type StaticGlbAsset } from "@moritzbrantner/three-d-renderer/static-glb"

import {
  loadSceneryManifest,
  SCENERY_PIECES,
  sceneryUrl,
  type SceneryManifest,
  type SceneryPiece,
} from "./scenery-manifest"

// The asset-tooling `zoo-scenery` pieces lowered through 3d-lab's static-GLB adapter. Adapted
// once per page and shared by every park-view mount; each mount's renderer owns its GPU copies.

export type SceneryKit = {
  manifest: SceneryManifest
  pieces: Record<SceneryPiece, StaticGlbAsset>
}

let pending: Promise<SceneryKit> | null = null

async function loadPiece(manifest: SceneryManifest, piece: SceneryPiece) {
  const entry = manifest.pieces[piece]
  const response = await fetch(sceneryUrl(entry.file))
  if (!response.ok) {
    throw new Error(`zoo-scenery/${entry.file}: HTTP ${response.status}`)
  }
  const asset = await adaptStaticGlb(await response.arrayBuffer())
  // The adapter keys geometry by the SHA-256 of the exact bytes; it must be the staged one.
  if (asset.resourceKey !== `glb-sha256:${entry.sha256}`) {
    throw new Error(`zoo-scenery/${entry.file} does not match its staged SHA-256`)
  }
  return [piece, asset] as const
}

/** Fetches and adapts the scenery pieces; rejects (and retries next call) on any mismatch. */
export function loadSceneryKit(): Promise<SceneryKit> {
  pending ??= (async () => {
    const manifest = await loadSceneryManifest()
    const pieces = await Promise.all(SCENERY_PIECES.map((piece) => loadPiece(manifest, piece)))
    return {
      manifest,
      pieces: Object.fromEntries(pieces) as Record<SceneryPiece, StaticGlbAsset>,
    }
  })()
  pending.catch(() => {
    pending = null
  })
  return pending
}
