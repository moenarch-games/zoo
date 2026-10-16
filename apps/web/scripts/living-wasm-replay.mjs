// Test adapter over generated Rust/WASM methods, not a second game executor.
import {readFileSync} from 'node:fs'
import {createHash} from 'node:crypto'
import init, {ZooGame} from '../src/wasm/zoo_core.js'

export async function verifyLivingWasmReplay(samples, assert) {
  const wasm = readFileSync(new URL('../src/wasm/zoo_core_bg.wasm',import.meta.url))
  await init({module_or_path:wasm})
  const wasmHash = createHash('sha256').update(wasm).digest('hex')
  const create = Reflect.get(ZooGame,'from_scenario_json')
  assert(typeof create === 'function', 'Existing Rust scenario loader must be exported through real WASM (#91); no test-only scenario executor')
  for (const viewport of ['desktop','phone']) {
    const rows = samples.filter(sample=>sample.viewport === viewport && sample.state)
    assert(rows.length > 0, 'Actual browser replay samples missing')
    const first = rows[0].state
    assert(first.replay?.scenarioJson && first.replay.coreWasmSha256 === wasmHash, 'Replay must use actual production core bytes and immutable scenario source')
    const game = create.call(ZooGame,first.replay.scenarioJson)
    const executed = []
    try {
      for (const row of rows) {
        const trace = row.state.replay
        assert(trace.scenarioJson === first.replay.scenarioJson && trace.coreWasmSha256 === wasmHash,
          'Scenario/WASM identity changed during actual browser journey')
        assert(Array.isArray(trace.commands) && trace.commands.length >= executed.length,
          'Actual command/tick trace is not append-only')
        assert(JSON.stringify(trace.commands.slice(0,executed.length)) === JSON.stringify(executed),
          'Earlier real command intent was changed in replay')
        for (const command of trace.commands.slice(executed.length)) {
          const method = Reflect.get(game,command.method)
          assert(Object.hasOwn(ZooGame.prototype,command.method) && typeof method === 'function' &&
            !['constructor','free','reset','snapshot_json','content_json'].includes(command.method),
            'Trace must invoke actual public Rust gameplay/tick methods')
          assert(Array.isArray(command.args), 'Real command arguments absent')
          const result = method.apply(game,command.args)
          if (command.result !== undefined) assert(result === command.result, 'Actual command receipt differs under camera-free replay')
          executed.push(command)
        }
        assert(JSON.stringify(JSON.parse(game.snapshot_json())) === JSON.stringify(row.state.simulation),
          'Camera/cosmetic browser journey changed actual Rust simulation versus same commands/ticks without rendering')
      }
    } finally { game.free() }
  }
}
