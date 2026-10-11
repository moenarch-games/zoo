import assert from "node:assert/strict"
import {readFileSync} from "node:fs"
import init, {ZooGame} from "../src/wasm/zoo_core.js"

await init({module_or_path: readFileSync(new URL("../src/wasm/zoo_core_bg.wasm", import.meta.url))})
const nativeOracle = JSON.parse(readFileSync(new URL("../../../crates/zoo-core/tests/fixtures/content-command-baseline.json", import.meta.url), "utf8"))
const definition = JSON.parse(new ZooGame().content_json())
for (const row of nativeOracle) {
  const game = ZooGame.from_content_json(JSON.stringify(definition))
  const accepted = (result: string) => assert.equal(JSON.parse(result).ok, true)
  accepted(game.place_habitat_rect(3, 8, 7, 10))
  accepted(game.hire_keeper())
  accepted(game.schedule_keeper(1))
  accepted(game.buy_animal_feed())
  accepted(game.adopt(1, row.species))
  accepted(game.place_concession(1, 6, "food"))
  accepted(game.place_concession(3, 6, "drink"))
  game.tick(137)
  const snapshot = JSON.parse(game.snapshot_json())
  for (const key of Object.keys(row.result)) assert.deepEqual(Object.hasOwn(snapshot, key) ? snapshot[key] : null, row.result[key])
  game.free()
}
const bad = structuredClone(definition)
bad.facilities[0].asset_ref = "missing-real-asset"
assert.throws(() => ZooGame.from_content_json(JSON.stringify(bad)), /asset_ref/)
console.log("content-wasm-acceptance=ok (real generated Rust bindings)")
