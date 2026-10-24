// Stages the pinned asset-tooling releases for the web build (#59 S2, #86): `zoo-scenery` meshes
// and `zoo-effects` particle flipbooks.
//
// Per release Zoo pins three things in assets/<dir>/: the release tag and tar SHA-256
// (<release>.pin.json), its own copy of the bundle's export.ref.json, and the release lineage.
// `zoo-effects` also pins its own copy of the release descriptor (atlas layouts and burst
// presets). This script downloads each tar (cached under .artifacts/), checks the pinned SHA-256,
// verifies it with asset-tooling's own `pack-asset-bundle.ts --verify --ref` at the pinned
// asset-tooling commit, and writes the files plus a provenance manifest to
// apps/web/public/<release>/. Any mismatch fails the build.
//
//   bun scripts/stage-asset-releases.ts
//
// ZOO_ASSET_TOOLING_DIR overrides the asset-tooling checkout (default .consumer-tools/asset-tooling,
// which CI checks out at the pinned commit; it is cloned there when missing).

import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { existsSync } from "node:fs"
import { mkdir, readFile, rm, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

type Pin = {
  release: string
  tarUrl: string
  tarSha256: string
  assetToolingCommit: string
  profile: string
  pieces: Record<string, { key: string; variant: string }>
}
type ManifestEntry = {
  key: string
  variant: string
  path: string
  source: { sha256: string; byteLength: number; mediaType: string }
}
type Lineage = {
  recipe: Record<string, string>
  provenance: string
  release: string
  parts: Record<string, { sha256: string; byteLength: number }>
}
type EffectDescriptor = {
  schema: string
  effects: {
    key: string
    variant: string
    role: string
    scale: number
    atlas: Record<string, unknown>
    png: { sha256: string; byteLength: number }
    presets: { standard: Record<string, unknown>; subtle: Record<string, unknown> }
  }[]
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..")
const sha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex")

function run(command: string, args: string[], cwd?: string) {
  const result = spawnSync(command, args, { cwd, stdio: ["ignore", "pipe", "inherit"] })
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed (exit ${result.status})`)
  }
  return result.stdout.toString().trim()
}

const readJson = async <T>(file: string) => JSON.parse(await readFile(file, "utf8")) as T

const releases = [
  { dir: path.join(root, "assets/scenery"), pinFile: "zoo-scenery.pin.json", extension: "glb" },
  { dir: path.join(root, "assets/effects"), pinFile: "zoo-effects.pin.json", extension: "png" },
] as const
const pins = await Promise.all(
  releases.map(({ dir, pinFile }) => readJson<Pin>(path.join(dir, pinFile))),
)
const commit = pins[0]!.assetToolingCommit
if (pins.some((pin) => pin.assetToolingCommit !== commit)) {
  throw new Error("zoo-scenery and zoo-effects must pin the same asset-tooling commit")
}

// The pinned asset-tooling checkout that verifies the tars.
const tooling = path.resolve(
  process.env.ZOO_ASSET_TOOLING_DIR ?? path.join(root, ".consumer-tools/asset-tooling"),
)
if (!existsSync(path.join(tooling, ".git"))) {
  await mkdir(tooling, { recursive: true })
  run("git", ["init", "--quiet"], tooling)
  run(
    "git",
    [
      "fetch",
      "--quiet",
      "--depth=1",
      "https://github.com/moritzbrantner/asset-tooling.git",
      commit,
    ],
    tooling,
  )
  run("git", ["checkout", "--quiet", "--detach", "FETCH_HEAD"], tooling)
}
const head = run("git", ["rev-parse", "HEAD"], tooling)
if (head !== commit) {
  throw new Error(
    `${tooling} is at ${head}, but the releases are pinned to asset-tooling ${commit}`,
  )
}
if (!existsSync(path.join(tooling, "node_modules"))) {
  run("bun", ["install", "--frozen-lockfile", "--silent"], tooling)
}
const { readAssetBundleTar } = (await import(path.join(tooling, "src/asset-bundle-pack.ts"))) as {
  readAssetBundleTar: (tar: Uint8Array) => { path: string; bytes: Uint8Array }[]
}

for (const [index, { dir, extension }] of releases.entries()) {
  const pin = pins[index]!
  const name = pin.release
  const lineage = await readJson<Lineage>(path.join(dir, "lineage.json"))
  const refPath = path.join(dir, "export.ref.json")
  if (lineage.release !== name) {
    throw new Error(`${name}: lineage.json is for release ${lineage.release}`)
  }

  // Download (or reuse) the tar and check the pin before anything reads it.
  const cache = path.join(root, `.artifacts/${name}`)
  const tarPath = path.join(cache, `${name}.tar`)
  await mkdir(cache, { recursive: true })
  let tar = existsSync(tarPath) ? new Uint8Array(await readFile(tarPath)) : null
  if (!tar || sha256(tar) !== pin.tarSha256) {
    const response = await fetch(pin.tarUrl)
    if (!response.ok) {
      throw new Error(`${name}: GET ${pin.tarUrl} returned HTTP ${response.status}`)
    }
    tar = new Uint8Array(await response.arrayBuffer())
    if (sha256(tar) !== pin.tarSha256) {
      throw new Error(
        `${name}: release tar sha256 ${sha256(tar)} does not match the pinned ${pin.tarSha256}`,
      )
    }
    await writeFile(tarPath, tar)
  }
  // The sidecar carries Zoo's pin, never the downloaded one.
  await writeFile(`${tarPath}.sha256`, `${pin.tarSha256}  ${name}.tar\n`)
  const verified = JSON.parse(
    run("bun", [
      path.join(tooling, "scripts/pack-asset-bundle.ts"),
      "--verify",
      tarPath,
      "--ref",
      refPath,
    ]),
  ) as { verified: boolean; profile: string; manifest: string }
  const ref = await readJson<{ sha256: string }>(refPath)
  if (!verified.verified || verified.profile !== pin.profile || verified.manifest !== ref.sha256) {
    throw new Error(`${name}: unexpected verification result ${JSON.stringify(verified)}`)
  }

  // Extract the verified closure with asset-tooling's own strict tar reader.
  const files = new Map(readAssetBundleTar(tar).map((entry) => [entry.path, entry.bytes]))
  const manifest = JSON.parse(Buffer.from(files.get("manifest.json")!).toString("utf8")) as {
    assets: ManifestEntry[]
  }
  const descriptor =
    extension === "png"
      ? await readJson<EffectDescriptor>(path.join(dir, `${name}.descriptor.json`))
      : null
  if (descriptor && descriptor.schema !== "zoo-effects-descriptor-v1") {
    throw new Error(`${name}: unsupported descriptor ${descriptor.schema}`)
  }

  const out = path.join(root, `apps/web/public/${name}`)
  await rm(out, { recursive: true, force: true })
  await mkdir(out, { recursive: true })
  const pieces: Record<string, Record<string, unknown>> = {}
  for (const [piece, { key, variant }] of Object.entries(pin.pieces)) {
    const entry = manifest.assets.find((asset) => asset.key === key && asset.variant === variant)
    if (!entry) {
      throw new Error(`${name}: the release has no ${key} variant ${variant}`)
    }
    const bytes = files.get(entry.path)!
    if (
      sha256(bytes) !== entry.source.sha256 ||
      lineage.parts[piece]?.sha256 !== entry.source.sha256
    ) {
      throw new Error(`${name}: ${piece} does not match the pinned lineage`)
    }
    const file = `${piece}.${extension}`
    await writeFile(path.join(out, file), bytes)
    pieces[piece] = { key, variant, file, sha256: entry.source.sha256, byteLength: bytes.length }
    if (descriptor) {
      const effect = descriptor.effects.find((e) => e.key === key && e.variant === variant)
      if (!effect || effect.png.sha256 !== entry.source.sha256) {
        throw new Error(`${name}: the pinned descriptor does not describe ${key}/${variant}`)
      }
      Object.assign(pieces[piece]!, {
        scale: effect.scale,
        atlas: effect.atlas,
        presets: effect.presets,
      })
    }
  }
  await writeFile(
    path.join(out, `${name.replace(/^zoo-/, "")}.json`),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        release: name,
        tarSha256: pin.tarSha256,
        manifestSha256: ref.sha256,
        assetToolingCommit: pin.assetToolingCommit,
        profile: pin.profile,
        recipe: lineage.recipe,
        provenance: lineage.provenance,
        pieces,
      },
      null,
      2,
    )}\n`,
  )
  console.log(
    `Staged ${Object.keys(pieces).length} ${name} pieces from release ${name} (tar ${pin.tarSha256.slice(0, 12)})`,
  )
}
