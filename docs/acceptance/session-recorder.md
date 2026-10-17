# Session recorder and failure recovery acceptance — #87

Authority: issue #87 and the owner answers recorded in the [Decision Atlas](../decisions/atlas.md), section 4.1 (decisions 4.1.1–4.1.5), together with `AGENTS.md` and `docs/architecture.md`. The baseline is default `56f06ca` ("docs: record owner decisions on session recorder (#87) and asset authority (#74)"). This contract was authored in an independent acceptance context before any production change under `crates/*/src` or `apps/web/src`. Shared conventions resolved without diagnostics at sourceRevision `5ad0db36974c8b0e5f9812c90ee34a4f227852c3`.

This is a Zoo recorder and recovery slice, not a fleet-wide observability framework. It does not export checkpoints: decision 4.1.1 defers checkpoint export to #88's versioned save format and forbids interim recorder-specific persistence. It adds no upload path and no Moonlight runtime-evidence route.

## Authority split

Decision 4.1.2 sets the split. `zoo-core` records the authoritative session, which covers the starting-state identity, the ordered command log with accepted and rejected results, events, work counters and simulation failures. It exposes that record through WASM. TypeScript adds only browser-side failures (WASM traps, renderer and asset failures, storage failures and uncaught application errors) and the recovery UI. TypeScript must not record commands, decide whether a command was accepted, or re-derive simulation state.

## Rust/WASM contract (`zoo_core::ZooGame`)

Existing methods keep their signatures. The contract adds exactly these public methods:

| Method | Contract |
| --- | --- |
| `diagnostics_json(&self) -> String` | Returns the session bundle described below as JSON, at most 1,048,576 bytes. It is read-only and is not recorded. |
| `session_status(&self) -> String` | Returns `"running"` or `"frozen"`. It is a cheap per-tick query for the UI and is not recorded. |
| `inject_fault(&mut self, kind: &str) -> String` | A test-only fault injector that returns an `ActionResult` JSON. `"invariant"` corrupts authoritative state so that the **real** invariant check detects the corruption no later than the next advance. `"panic"` makes the next advance panic; in WASM the panic is a trap. Any other kind returns `ok: false` and changes nothing. The call is recorded as a command (`kind: "inject_fault"`, `args: {"kind": …}`), so a replay reproduces the failing step. Normal play never calls it (see Fault injection). |
| `ZooGame::replay(content_document: &str, session_json: &str) -> Result<ZooGame, String>` | Builds a new game from the content document and replays the bundle's `commands` in `seq` order. It returns `Err` in four cases: the content does not match `start` (`ruleset_id`, `schema_version` or `content_digest`); the log is incomplete (`commands_dropped > 0`, or the first `seq` is not 0); any re-executed result differs from the recorded `result` (a divergence); or the bundle is malformed. The replayed game records its own session, which must equal the original. |

The existing behaviour changes in three ways.

- **Recorded commands.** Every authoritative mutation is appended to the log with its result: `place_path`, `place_concession`, `place_habitat_rect`, `place_habitat`, `bulldoze`, `adopt`, `buy_animal_feed`, `hire_keeper`, `hire_janitor`, `hire_mechanic`, `schedule_keeper`, `feed_habitat`, `refill_water`, `clean_habitat`, `add_shelter`, `tick` (as `advance`) and `inject_fault`. Read-only queries and previews are never recorded: `snapshot_json`, `content_json`, `evaluate_habitat`, `evaluate_habitat_rect`, `diagnostics_json` and `session_status`. Camera, selection, hover, audio and other cosmetic actions never reach `zoo-core`, so they cannot enter the log.
- **Frozen sessions.** Once a simulation failure is detected, the session is `frozen`. `tick` becomes a no-op. Every mutating command returns `{"ok": false, "message": "…frozen…"}` without changing state, and it may still be recorded. The snapshot, clock and failure list stay fixed: one failure is recorded, and no failure is re-detected on later calls. Nothing resets the park automatically.
- **Reset.** `reset()`, which is the player's explicit New park action, starts a fresh session from the same identified initial state, with an empty log, no failures and tick 0. It is the only way out of a frozen session.

### Command entry arguments

The `args` object of each entry uses the method's parameter names. Examples: `{"x","y"}`, `{"x","y","kind"}`, `{"ax","ay","bx","by"}`, `{"x","y","orientation"}`, `{"habitat_id","species"}`, `{"habitat_id"}` and `{}`. An advance is recorded as `{"kind": "advance", "args": {"minutes": n}}`, where `n` is the number of minutes actually simulated, after the existing 240-minute clamp. Consecutive advances **may** be merged into one entry with the summed minutes, stamped with the tick of the first. A merged entry must replay identically; consecutive advances are covered by the replay test. Merging is recommended, so that the 4 Hz browser tick does not exhaust the 2,000-entry ring.

### Bundle (`diagnostics_json`)

The bundle has a closed field set. It has exactly the fields below, and each nested object has exactly its listed fields. It contains no credentials, no URLs, no file paths and no browser data.

| Field | Meaning |
| --- | --- |
| `format` | `"zoo-diagnostics/1"` |
| `build` | `{zoo_core_version}`: the crate version. |
| `start` | `{scenario, ruleset_id, schema_version, content_digest}`, the starting-state identity. `scenario` is `"default"` until #91 adds scenarios. `content_digest` is a deterministic, non-empty digest of the validated content; content that differs must produce a different digest. The simulation has no RNG, so there is no seed field. |
| `status` | `"running"` or `"frozen"`. |
| `clock` | `{tick, day, minute_of_day}`. `tick` is the number of simulated minutes since the session started. `day` and `minute_of_day` equal the snapshot's values. |
| `limits` | `{max_commands: 2000, max_events: 500, max_export_bytes: 1048576}` |
| `commands` | The most recent ≤2,000 entries, each `{seq, tick, kind, args, result: {ok, message}}`. `seq` is 0-based and contiguous over the whole session. `tick` is the tick at which the command was issued. |
| `commands_total`, `commands_dropped` | `commands_total` counts every entry in the session. `commands_dropped = commands_total − commands.len()`. |
| `events` | The most recent ≤500 entries, each `{seq, tick, kind, entity_id}`, in non-decreasing tick order. `seq` is contiguous. `entity_id` is a stable game ID or `null`. At least `guest_arrived` and `guest_left` are emitted, carrying the guest's stable ID; other kinds are allowed. |
| `events_total`, `events_dropped` | The same as the command counters, for events. |
| `entities` | `{habitats, concessions, keepers, janitors, mechanics, guests}`: sorted arrays of the current stable IDs. |
| `work` | Named, non-negative integer counters that only increase during a session. They include `simulated_minutes` (equal to `clock.tick`), `guest_updates` and `path_searches`, which are the counters the performance contract planned (`.performance/contract.json`). Further counters may be added, but every value must be an integer. |
| `failures` | Simulation failures, each `{seq, tick, source: "simulation", kind, message, injected}`. `kind` is `"invariant_violation"` or `"panic"`. `seq` is the recorded command during which the failure was detected. `injected` is `true` only when `inject_fault` caused the failure. |
| `truncated` | `true` when anything was shortened or omitted to respect the 1 MB cap, such as long argument strings or the oldest entries. The latest command must still be present. |

### Bounds (Atlas 4.1.4)

- **Commands:** a ring of 2,000 entries.
- **Events:** a ring of 500 entries.
- **Export:** at most 1,048,576 bytes, for both the Rust bundle and the browser export document.
- **Screenshot:** optional (Atlas 4.1.5). At most one PNG data URL of at most 262,144 characters. It is captured only on failure or export, never continuously.
- **Browser failures:** a ring of 50 entries, with no single message over 2,000 characters.
- **Memory:** internal memory must stay bounded by these rings. A full-history buffer must not hide behind a bounded export.

## Browser contract (`apps/web`)

### `window.__zooDiagnostics`

The app always installs `window.__zooDiagnostics` once the park has loaded. Calling `exportText()` returns the exact text that Download and Copy export. That text is a JSON document with exactly these fields:

- `session`: the latest Rust bundle.
- `browser_failures`: an array of `{source, message, recovery, tick}`. `source` is one of `"wasm" | "renderer" | "asset" | "storage" | "application"`. `recovery` is one of `"frozen" | "continue_paused" | "manual_export"`. `tick` is a number or `null`.
- `screenshot`: `null` or a size-capped `data:image/png;base64,…` URL.

The document must not contain cookies, storage contents, URL query values, the user agent or any other unrelated browser data, and nothing is ever sent over the network.

After a WASM trap, the wasm-bindgen object can be left unusable, because of an outstanding borrow or a poisoned instance. `exportText()` must still return the latest retained session bundle together with the `wasm` failure. How the bundle is retained is the implementer's choice, but it must stay within the bounds above and must not serialize the full bundle on every tick.

### Recovery and export panel

The panel is one compact element with `aria-label="Park recovery"` (a non-modal dialog or region). It covers less than 40% of the viewport, so the park remains visible. It always offers buttons named **Download diagnostics** and **Copy diagnostics**. Download is a file the player saves by clicking, never an automatic download; Copy writes `exportText()` to the clipboard. When Copy or Download fails because clipboard, storage or download access is denied, the panel records a `storage` failure with recovery `manual_export`. It then shows the bundle in a read-only, selectable `textarea[aria-label="Diagnostics bundle"]` and reports the failure visibly.

| Trigger | Simulation | Panel |
| --- | --- | --- |
| WASM panic or trap from any call | Stops advancing: the interval stops and later calls are not attempted. A `wasm`/`frozen` browser failure is recorded. | Export controls only. No Continue paused. |
| Invariant violation detected by `zoo-core` (`session_status() == "frozen"`) | Frozen in Rust. | Export controls only. No Continue paused. |
| Renderer failure (startup failure, a render exception, or WebGL context loss) or missing-asset failure | Paused, with Rust state intact (`status: "running"`). | Export controls plus **Continue paused**. Choosing it closes the panel and leaves the simulation paused until the player picks a speed. The failed renderer stays failed closed and does not reopen the panel. |
| Uncaught application error or unhandled rejection | Stops advancing. An `application` failure is recorded with the error message. | Export controls. Whether Continue paused is offered is unspecified. |
| Storage, clipboard or download denial | Unaffected; boot must not depend on storage. | Manual export fallback, as described above. |
| Normal play | Unaffected. | A compact **Export diagnostics** control opens the same panel without a failure. That panel has a **Close** control and no Continue paused. |

Failure handling must not loop. Each failure is recorded once and opens at most one panel. The 250 ms tick does not retry a trapped or frozen simulation. Repeated failing exports produce bounded records.

### Fault injection (test-only)

The production build is the build under test, because Verify runs the proof against `bun run preview`. Fault injection is therefore gated at run time. `window.__zooFaults` is installed **only** when the page URL carries `?zoo-fault-injection=1`, and normal play never has it. `window.__zooFaults.simulation(kind)` calls `ZooGame.inject_fault(kind)`, with `"panic"` or `"invariant"`. The proof injects renderer, application and storage failures itself, at the real browser boundary: WebGL context loss through `WEBGL_lose_context`, a thrown `setTimeout` error, and throwing `localStorage`/`sessionStorage`/`indexedDB` and `clipboard.writeText` installed before load, with downloads denied. None of these needs an app hook. Injected simulation faults are labelled `injected: true` and appear in the command log.

## Executable evidence

### Native: `crates/zoo-core/tests/session_recorder.rs`

The 13 tests use only the public API above.

| Test | Proves |
| --- | --- |
| `bundle_declares_build_start_identity_clock_limits_and_only_declared_fields` | The closed field set, build and content identity, the clock against the snapshot, the documented limits, and counters that are integers only. |
| `start_identity_digest_follows_validated_content` | The starting-state identity is stable, and it changes when the content changes. |
| `accepted_and_rejected_commands_are_recorded_in_order_with_results` | Both outcomes are recorded with `seq`, `tick`, `kind`, `args` and the exact result message, and `advance` is recorded. |
| `queries_and_previews_are_not_authoritative_commands` | Snapshots, previews and diagnostics are not logged, and no camera or cosmetic kind appears. |
| `replay_from_start_identity_and_ordered_log_reproduces_final_state_and_failing_step` | Two scripted runs give byte-identical bundles. Three replays reproduce the snapshot, log, clock and failure (same `seq`) identically. A healthy session replays to the same final state. |
| `replay_fails_closed_on_identity_mismatch_divergence_or_incomplete_log` | Replay rejects mismatched content, a tampered result and a dropped log. |
| `command_ring_keeps_exactly_the_latest_two_thousand` | After 2,500 commands it retains exactly 2,000, with contiguous trailing `seq` values and correct totals. |
| `event_ring_keeps_latest_five_hundred_with_stable_entity_ids` | After six simulated days (more than 500 events) it retains exactly 500. Every current guest ID has a `guest_arrived` event, and `entities` matches the snapshot. |
| `work_counters_track_simulated_subsystem_work` | `simulated_minutes`, `guest_updates` and `path_searches` exist and are non-zero after work. No counter decreases. |
| `export_stays_within_one_megabyte_and_keeps_the_latest_command` | 2,100 commands with 4 KiB arguments plus long advances still produce at most 1 MB, with `truncated: true` and the latest command retained. A small session has `truncated: false`. |
| `detected_invariant_violation_freezes_without_looping_or_resetting` | It freezes and records exactly one failure that names its command. Fifty further 240-minute ticks change nothing. Commands are refused as frozen. The park is not reset. |
| `fault_injection_rejects_unknown_kinds_and_explicit_reset_starts_a_new_session` | An unknown fault kind is refused. Only an explicit `reset()` leaves a frozen session, and it starts a fresh, identically identified session. |
| `bundle_contains_no_credentials_or_external_data` | No cookie, token, password, secret, user-agent, URL or path text appears. |

A WASM *panic* cannot be observed natively, because there is no unwinding on `wasm32` and the crate does not require `catch_unwind`. It is covered by the browser proof.

### Browser: `apps/web/scripts/session-recorder-proof.mjs`

`capture-park-view-proof.mjs` calls this script after its existing journey has passed. That journey is unchanged and is still asserted first. Verify's "Browser visual dogfood" step therefore already runs the new script, and **no workflow edit is required**. Each phase loads a fresh page of the production preview at desktop size.

1. **Normal play.** `__zooFaults` is absent. A canvas-picked accepted path and a rejected fence both appear in the exported log with their results. Orbit, zoom and reset add no command. The **Export diagnostics** control opens a compact panel with Download and Copy, without Continue paused. **Close** closes it, and the session keeps running.
2. **WASM panic** (`?zoo-fault-injection=1`, `__zooFaults.simulation("panic")` at 4×).
   - **Panel and clock.** A compact panel appears without Continue paused, and the HUD clock stays still even when 4× is pressed.
   - **Export.** The export holds a `wasm`/`frozen` failure and the earlier accepted and rejected commands, contains only the declared fields, and is at most 1 MB.
   - **Privacy.** The planted cookie, localStorage value and query value do not appear in the export.
   - **Bounded handling.** Over 2 s there is no growth in failure records, at most two new console errors, and exactly one panel.
   - **Download and Copy.** Download produces exactly one file, and Copy writes clipboard text. Both equal `exportText()`.
   - **No uploads.** After the failure the page makes no non-GET request and no request to another origin.
3. **Invariant violation.** The panel appears without Continue paused, and the clock is frozen. `session.status` is `"frozen"`, with exactly one `simulation`/`invariant_violation`/`injected` failure and the command log. Nothing loops.
4. **Renderer failure: real WebGL context loss.** The panel offers Continue paused, and the simulation is paused while the panel is open. The export holds a `renderer`/`continue_paused` failure, the Rust session is still running with no failures, and the log is present. Nothing loops. After Continue paused the panel closes and stays closed, the renderer failure is recorded once, and choosing 1× resumes the clock.
5. **Uncaught application error.** The panel appears, the clock stops, and an `application` failure carries the error message. Nothing loops.
6. **Storage failure: throwing storage and clipboard, downloads denied.** The park still boots. A failing Copy shows the manual `Diagnostics bundle` textarea holding the bundle and records a `storage`/`manual_export` failure. Retries produce bounded records, and the simulation keeps running.

The script saves `test-results/session-recovery-{wasm-panic,invariant,renderer,storage}.png`. The implementer may add these files to the Verify artifact upload list. That is the only workflow edit this contract anticipates, and it is optional.

## Run

```sh
make verify                                            # includes cargo test --workspace (new tests)
cargo test -p zoo-core --test session_recorder --locked
cd apps/web && bun run preview -- --host 127.0.0.1 --port 4173 &   # after make verify built dist/
cd apps/web && bun scripts/capture-park-view-proof.mjs              # existing journey + #87 phases
```

The merge gate runs every case above on the PR head, together with the complete `make verify` and the hosted Verify workflow. A RED baseline does not waive any later failure.

## Obligation mapping

| Obligation (issue #87 / Atlas 4.1) | Evidence |
| --- | --- |
| Bounded recorder: build/content versions, starting-state identity, tick, accepted/rejected commands, stable IDs, recent events, work counters | Native: `bundle_declares_*`, `start_identity_*`, `accepted_and_rejected_*`, `event_ring_*`, `work_counters_*` |
| Recording starts automatically when a park starts | Native: a new game, `from_content_json` and `reset` begin recording with no extra call (`accepted_and_rejected_*`, `fault_injection_*`). Browser phase 1. |
| On a WASM error, uncaught failure or invariant violation, stop advancing safely and retain the bundle | Native: `detected_invariant_violation_*`. Browser phases 2, 3 and 5. |
| Explicit export action; same bundle exposed to automated proofs; panel instead of automatic download | Browser phases 1, 2 and 6 (Download and Copy equal `exportText()`; manual fallback). |
| Checkpoint export "when supported" | **Deferred to #88** by decision 4.1.1. No check. A later #88 contract adds it. |
| Reproduction from the initial state and ordered commands; deterministic same failing step or final state | Native: `replay_from_start_identity_*`, `replay_fails_closed_*` |
| Camera, audio and cosmetic events kept out of the authoritative log and RNG | Native: `queries_and_previews_*`. Browser phase 1 (camera adds no command). There is no RNG to separate. |
| Honest failure classes with different recovery; no silent reset | Browser phases 2–6 (frozen, continue_paused or manual_export, per source). Native: `detected_invariant_violation_*` (no reset). |
| Injected WASM, rendering and storage failures give actionable diagnostics; handling cannot loop | Browser phases 2, 3, 4 and 6 (no-loop checks). Native freeze test (one failure after 50 ticks). |
| Documented limits; no unbounded growth in long sessions | This spec (Bounds). Native: `command_ring_*`, `event_ring_*`, `export_stays_*`. The internal memory bound is a review item. |
| No credentials or unrelated browser data; nothing uploaded automatically | Native: `bundle_contains_no_credentials_*` and the closed field sets. Browser phase 2 (planted secrets, network watch). |
| The issue report records scenario/seed, expected vs actual, and a screenshot when visual; headless is distinguished from physical-device testing | The PR and issue report must state these (review item). `start.scenario` is in the bundle, and there is no seed because there is no RNG. Physical-device evidence is a **gap**. |
| Normal play keeps the cheap verification path; expensive tracing is opt-in except for the bounded buffer | The existing journey runs first, unchanged (browser). Review item for cost. |
| Atlas 4.1.3 compact panel; renderer and asset failures continue paused | Browser phases 2–4 (panel under 40% of the viewport; Continue paused only for the renderer). Asset failure is partly a gap (below). |
| Atlas 4.1.5 optional size-capped screenshot | Browser export-shape check (`null` or a PNG data URL of at most 256 KiB characters). |

## Independent review items (not mechanically checked)

- **Real invariant check.** `inject_fault("invariant")` must corrupt real state, for example a duplicated stable ID or cash outside `MAX_CENTS`, and detection must come from an invariant check that runs in normal play. A flag that only pretends to fail is not acceptable. The check must be cheap: at most linear in the entities per advance.
- **Bounded memory.** No unbounded internal history, and no full-bundle serialization per tick. The recovery buffer is the only always-on recording.
- **Boundary.** TypeScript does not log commands or interpret results, which decision 4.1.2 assigns to `zoo-core`. `__zooFaults` is reachable only with the query parameter.
- **Merged advances.** Coalesced advances, if used, keep replay identical.
- **PR evidence.** The PR records the expected and actual behaviour per failure class, the recovery screenshots, and the fact that the evidence comes from headless Chromium with SwiftShader.
- **Clippy.** The new test file was formatted with `cargo fmt` but cannot be linted until the API exists. Non-semantic Clippy fixes to it are allowed. Any change to its assertions requires independent re-verification.

## Evidence gaps

- **Physical devices.** All browser evidence is headless Chromium with SwiftShader. Real GPU context loss, mobile Safari download and clipboard behaviour, and phone-sized recovery-panel layout on a physical device are unverified.
- **Missing assets at run time.** The park has no runtime asset loading to fail, and an invalid `asset_ref` is rejected at boot by the existing content acceptance, which shows no fallback park. The asset row of the recovery table is therefore checked only by review until runtime asset loading exists.
- **Checkpoints.** Deferred to #88. A session whose log has dropped entries (`commands_dropped > 0`) cannot be replayed from the initial state, and `ZooGame::replay` refuses it.
- **WASM panic natively.** Covered only in the browser.

## Baseline observations (`56f06ca`, branch `agent/87-session-recorder`)

- **`make verify` (without the new test file): exit 0.** Format, WASM build, Clippy, Oxlint, the workspace tests (98 passed: zoo-core 52 unit, 15 content contract, 1 content baseline, 1 viewing capacity, 1 visitor engagement and 14 routing; zoo-scene 14), the type check and the production web build all passed.
- **`cargo test -p zoo-core --test session_recorder --locked`: RED.** Compilation fails with 26 `E0599` errors: `diagnostics_json` ×11, `session_status` ×6, `replay` ×5 and `inject_fault` ×4. Those are the missing public API, and there are no other errors. `make verify` with the file present is therefore RED at lint and test.
- **`bun scripts/capture-park-view-proof.mjs` against `bun run preview`: the existing journey is GREEN, and the #87 phases are RED.** The existing journey printed "Park view dogfood passed: …". The #87 phase then failed with `timed out waiting for window.__zooDiagnostics: {"renderer":"ready","diagnosticsHook":"undefined","faultHook":"undefined","panel":null}` and exit 1.
- **Harness feasibility at baseline, using a throwaway probe.** Headless Chromium supports clipboard write and read with granted permissions, CDP `allowAndName` downloads, and `WEBGL_lose_context`. After context loss, `isContextLost()` is `true` but the park view still reports `data-renderer="ready"`: the baseline does not detect the failure. The park boots with throwing storage and clipboard APIs.
