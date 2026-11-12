import {
  adaptImportedCharacterBundle,
  type ImportedCharacterAsset,
} from "@moritzbrantner/three-d-renderer/imported-character"

// The asset-tooling `zoo-animals` characters staged by scripts/stage-asset-releases.ts (#112):
// rigid node-animated expressive animals with idle/walk/feed/rest clips, loaded through 3d-lab's
// imported-character bundle adapter (whose Rust runtime owns clip sampling and poses). The staged
// bytes are fetched and checked against the bundle manifest's SHA-256 once per page; each
// park-view mount adapts its own assets and disposes them with its renderer.

/** The clips the park view plays; every pinned animal must carry all of them. */
export const ANIMAL_CLIPS = ["idle", "walk", "rest"] as const
export type AnimalClip = (typeof ANIMAL_CLIPS)[number]

export type AnimalsManifest = {
  schemaVersion: 1
  release: string
  tarSha256: string
  manifestSha256: string
  assetToolingCommit: string
  profile: string
  license: { spdx: string; attribution: string; evidenceUrl: string }
  provenance: string
  /** Keyed by the species' validated presentation `asset_ref`. */
  pieces: Record<
    string,
    {
      key: string
      variant: string
      sceneSha256: string
      /** Full-size height in metres, from the release lineage. */
      height: number
      clips: string[]
      issue: string
      recipe: Record<string, string>
    }
  >
}

type BundleManifest = {
  profile: string
  assets: {
    key: string
    variant: string
    path: string
    sha256: string
    resources: { path: string; sha256: string }[]
  }[]
}

export type AnimalCharacter = {
  asset: ImportedCharacterAsset
  /** Full-size height in metres; the park scales every animal uniformly. */
  height: number
  /** Clip name → duration in seconds. */
  clips: ReadonlyMap<string, number>
}

export type AnimalKit = {
  manifest: AnimalsManifest
  /** Keyed by the species' presentation `asset_ref`. */
  characters: ReadonlyMap<string, AnimalCharacter>
  dispose(): void
}

const animalsUrl = (file: string) => new URL(`zoo-animals/${file}`, document.baseURI)

async function fetchBytes(file: string) {
  const response = await fetch(animalsUrl(file))
  if (!response.ok) {
    throw new Error(`zoo-animals/${file}: HTTP ${response.status}`)
  }
  return new Uint8Array(await response.arrayBuffer())
}

async function sha256Hex(bytes: Uint8Array<ArrayBuffer>) {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("")
}

type Staged = {
  manifest: AnimalsManifest
  bundle: BundleManifest
  /** Bundle-relative path → verified bytes. */
  files: ReadonlyMap<string, Uint8Array>
}

let pending: Promise<Staged> | null = null

/** Fetches the staged animals once per page and checks every byte against the bundle manifest. */
function loadStaged(): Promise<Staged> {
  pending ??= (async () => {
    const manifest = JSON.parse(
      new TextDecoder().decode(await fetchBytes("animals.json")),
    ) as AnimalsManifest
    if (manifest.schemaVersion !== 1 || manifest.profile !== "gltf-resources-v1") {
      throw new Error("zoo-animals/animals.json is not a staged gltf-resources-v1 release")
    }
    const bundleBytes = await fetchBytes("manifest.json")
    if ((await sha256Hex(bundleBytes)) !== manifest.manifestSha256) {
      throw new Error("zoo-animals/manifest.json does not match its staged SHA-256")
    }
    const bundle = JSON.parse(new TextDecoder().decode(bundleBytes)) as BundleManifest
    const files = new Map<string, Uint8Array>()
    const fetchVerified = async (file: string, sha256: string) => {
      const bytes = await fetchBytes(file)
      if ((await sha256Hex(bytes)) !== sha256) {
        throw new Error(`zoo-animals/${file} does not match its manifest SHA-256`)
      }
      files.set(file, bytes)
    }
    await Promise.all(
      Object.entries(manifest.pieces).map(async ([piece, { key, variant, sceneSha256, clips }]) => {
        const entry = bundle.assets.find((asset) => asset.key === key && asset.variant === variant)
        if (!entry || entry.sha256 !== sceneSha256) {
          throw new Error(`zoo-animals: ${piece} is not the staged ${key}/${variant}`)
        }
        const missing = ANIMAL_CLIPS.filter((clip) => !clips.includes(clip))
        if (missing.length > 0) {
          throw new Error(`zoo-animals: ${piece} lacks the ${missing.join(", ")} clips`)
        }
        await Promise.all([
          fetchVerified(entry.path, entry.sha256),
          ...entry.resources.map((resource) => fetchVerified(resource.path, resource.sha256)),
        ])
      }),
    )
    return { manifest, bundle, files }
  })()
  pending.catch(() => {
    pending = null
  })
  return pending
}

/**
 * Adapts every staged animal for one park-view mount. Rejects (disposing what it adapted) when an
 * asset is missing, mismatched or lacks a clip the park plays; the caller owns `dispose()`.
 */
export async function loadAnimalKit(): Promise<AnimalKit> {
  const { manifest, bundle, files } = await loadStaged()
  const readResource = (file: string) => {
    const bytes = files.get(file)
    if (!bytes) {
      return Promise.reject(new Error(`zoo-animals/${file} was not staged`))
    }
    return Promise.resolve(bytes)
  }
  const characters = new Map<string, AnimalCharacter>()
  const dispose = () => {
    for (const character of characters.values()) {
      character.asset.dispose()
    }
    characters.clear()
  }
  try {
    for (const [piece, { key, variant, sceneSha256, height }] of Object.entries(manifest.pieces)) {
      const asset = await adaptImportedCharacterBundle({
        manifest: bundle,
        key,
        variant,
        readResource,
      })
      characters.set(piece, {
        asset,
        height,
        clips: new Map(asset.clips.map((clip) => [clip.name, clip.duration])),
      })
      if (asset.resourceKey !== `bundle:${key}/${variant}:${sceneSha256}`) {
        throw new Error(`zoo-animals: ${piece} adapted as ${asset.resourceKey}`)
      }
      const missing = ANIMAL_CLIPS.filter((clip) => !asset.clips.some((c) => c.name === clip))
      if (missing.length > 0) {
        throw new Error(`zoo-animals: ${piece} has no ${missing.join(", ")} clip at runtime`)
      }
    }
  } catch (error) {
    dispose()
    throw error
  }
  return { manifest, characters, dispose }
}
