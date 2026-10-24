// Stages the pinned asset-tooling `zoo-scenery` release for the web build (#59 S2, #86).
//
// Zoo pins three things in assets/scenery/: the release tag and tar SHA-256 (zoo-scenery.pin.json),
// its own copy of the bundle's export.ref.json, and the release lineage. This script downloads the
// tar (cached under .artifacts/), checks the pinned SHA-256, verifies it with asset-tooling's own
// `pack-asset-bundle.ts --verify --ref` at the pinned asset-tooling commit, and writes the GLBs plus
// a provenance manifest to apps/web/public/zoo-scenery/. Any mismatch fails the build.
//
//   bun scripts/stage-zoo-scenery.ts
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
  recipe: { script: string; sha256: string; blenderVersion: string }
  provenance: string
  release: string
  parts: Record<string, { sha256: string; byteLength: number }>
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..")
const pinDir = path.join(root, "assets/scenery")
const pin = JSON.parse(await readFile(path.join(pinDir, "zoo-scenery.pin.json"), "utf8")) as Pin
const lineage = JSON.parse(await readFile(path.join(pinDir, "lineage.json"), "utf8")) as Lineage
const refPath = path.join(pinDir, "export.ref.json")
const sha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex")

function run(command: string, args: string[], cwd?: string) {
  const result = spawnSync(command, args, { cwd, stdio: ["ignore", "pipe", "inherit"] })
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed (exit ${result.status})`)
  }
  return result.stdout.toString().trim()
}

// The pinned asset-tooling checkout that verifies the tar.
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
      pin.assetToolingCommit,
    ],
    tooling,
  )
  run("git", ["checkout", "--quiet", "--detach", "FETCH_HEAD"], tooling)
}
const head = run("git", ["rev-parse", "HEAD"], tooling)
if (head !== pin.assetToolingCommit) {
  throw new Error(
    `${tooling} is at ${head}, but zoo-scenery is pinned to asset-tooling ${pin.assetToolingCommit}`,
  )
}
if (!existsSync(path.join(tooling, "node_modules"))) {
  run("bun", ["install", "--frozen-lockfile", "--silent"], tooling)
}

// Download (or reuse) the tar and check the pin before anything reads it.
const cache = path.join(root, ".artifacts/zoo-scenery")
const tarPath = path.join(cache, "zoo-scenery.tar")
await mkdir(cache, { recursive: true })
let tar = existsSync(tarPath) ? new Uint8Array(await readFile(tarPath)) : null
if (!tar || sha256(tar) !== pin.tarSha256) {
  const response = await fetch(pin.tarUrl)
  if (!response.ok) {
    throw new Error(`zoo-scenery: GET ${pin.tarUrl} returned HTTP ${response.status}`)
  }
  tar = new Uint8Array(await response.arrayBuffer())
  if (sha256(tar) !== pin.tarSha256) {
    throw new Error(
      `zoo-scenery: release tar sha256 ${sha256(tar)} does not match the pinned ${pin.tarSha256}`,
    )
  }
  await writeFile(tarPath, tar)
}
// The sidecar carries Zoo's pin, never the downloaded one.
await writeFile(`${tarPath}.sha256`, `${pin.tarSha256}  zoo-scenery.tar\n`)
const verified = JSON.parse(
  run("bun", [
    path.join(tooling, "scripts/pack-asset-bundle.ts"),
    "--verify",
    tarPath,
    "--ref",
    refPath,
  ]),
) as { verified: boolean; profile: string; manifest: string }
const ref = JSON.parse(await readFile(refPath, "utf8")) as { sha256: string }
if (!verified.verified || verified.profile !== pin.profile || verified.manifest !== ref.sha256) {
  throw new Error(`zoo-scenery: unexpected verification result ${JSON.stringify(verified)}`)
}

// Extract the verified closure with asset-tooling's own strict tar reader.
const { readAssetBundleTar } = (await import(path.join(tooling, "src/asset-bundle-pack.ts"))) as {
  readAssetBundleTar: (tar: Uint8Array) => { path: string; bytes: Uint8Array }[]
}
const files = new Map(readAssetBundleTar(tar).map((entry) => [entry.path, entry.bytes]))
const manifest = JSON.parse(Buffer.from(files.get("manifest.json")!).toString("utf8")) as {
  assets: ManifestEntry[]
}

const out = path.join(root, "apps/web/public/zoo-scenery")
await rm(out, { recursive: true, force: true })
await mkdir(out, { recursive: true })
const pieces: Record<
  string,
  { key: string; variant: string; file: string; sha256: string; byteLength: number }
> = {}
for (const [name, { key, variant }] of Object.entries(pin.pieces)) {
  const entry = manifest.assets.find((asset) => asset.key === key && asset.variant === variant)
  if (!entry) {
    throw new Error(`zoo-scenery: the release has no ${key} variant ${variant}`)
  }
  const bytes = files.get(entry.path)!
  const part = lineage.parts[name]
  if (sha256(bytes) !== entry.source.sha256 || part?.sha256 !== entry.source.sha256) {
    throw new Error(`zoo-scenery: ${name} does not match the pinned lineage`)
  }
  const file = `${name}.glb`
  await writeFile(path.join(out, file), bytes)
  pieces[name] = { key, variant, file, sha256: entry.source.sha256, byteLength: bytes.length }
}
await writeFile(
  path.join(out, "scenery.json"),
  `${JSON.stringify(
    {
      schemaVersion: 1,
      release: pin.release,
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
  `Staged ${Object.keys(pieces).length} zoo-scenery pieces from release ${pin.release} (tar ${pin.tarSha256.slice(0, 12)})`,
)
