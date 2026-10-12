# Living habitat browser acceptance

Independent acceptance for #140, derived from the owner specification in
PR #139 at `8a5ad562bce46436afb016040c83446f079b103c` and #142.
Source baseline: `a5938e5` (origin/main). No production changes are included.
Shared conventions sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.

Run `make web`, start `bun run preview -- --host 127.0.0.1 --port 4173`
in `apps/web`, then run `LIVING_HABITAT_PROOF=1 bun scripts/capture-park-view-proof.mjs`.
The existing construction, invalid-placement, orbit and touch tests execute first.
The extension reloads a clean page for each viewport and uses real pointer/touch
input. The authored living scenario is selected through its visible UI.
Baseline failure is expected: scenario/individual activity functionality is absent.
Do not skip the extension to approve #142.

## Observation seam for implementation handoff

Extend the existing read-only `window.__zooParkView` with `livingHabitat()`.
Return current values derived from the actual WASM snapshot and renderer:
`scenario: {id, kind, seed}`, `tick`, `objective: {id, target, progress}`,
`cameraObjectiveId`, `animals: [{id, name, activity, position}]`,
`guests: [{id, state, viewingPoint, habitatId}]`,
`assets: [{loaded, provenance, placeholder}]`, `selectedAnimalId`,
`followAnimalId` and `simulation` (the complete authoritative snapshot).
Positions are renderer world positions consumed by existing `projectWorld`.
Activity values are `Walking`, `Feeding`, `Resting`; adapters may translate real
Rust enum spelling without inventing behavior. The selected fixture supplies all
three activities; numerical game thresholds and exact species are not prescribed.

UI selectors are semantic hooks, not product wording: `[data-scenario-kind="living"]`,
`[data-action="pause"]`, `[data-action="resume"]`,
`[data-action="release-follow"]`, `[data-objective-id]`, `[data-animal-watch]`.
No setter, fake state injection, mock simulation or scripted success flag belongs
in this seam. The implementation acceptance reviewer must verify snapshot provenance,
renderer asset/node correspondence, first-objective framing and guest viewing semantics;
self-reported fields alone cannot prove these architectural facts.

## Evidence and limitations

CI uploads opening/individual/activity frames on desktop and phone, baseline frames
on failure, and JSON scenario/step/state metadata. Sequential activity images provide
an equivalent visual proof sequence; no claim of a video recording is made.
Runtime activity synchronization uses authoritative ticks rather than a fixed sleep.
Paused camera-only orbit/follow/release must preserve the full simulation snapshot.
The independently reconciled watch-objective exception below applies only to a
real observation command, not camera interpolation.

Automated success does **not** establish owner visual approval. The owner must review
normal and focused frames for miniature-world readability, expressive recognizable
animals, themed habitat, visible guest reactions, compact objectives and toolbar.
A placeholder detector and truthful provenance review complement this visual gate;
assertions cannot reliably judge appealing art or a smooth camera trajectory.

Remaining evidence required before #142 completion: actual follow camera tracking
of a moving individual, authoritative objective progression following an owner action,
valid guest approach/reaction transitions, building panel usability while the park
stays visible, production desktop/phone functional pass and owner visual signoff.
This acceptance-only draft must remain unmerged while the living journey is red.

The extension retains the established `.mjs` CDP runner format as a narrow
compatibility exception to TS-001. It does not introduce an application module or
a second test harness. Syntax validation uses `node --check`; executable browser
coverage is the meaningful gate. Future TypeScript migration must preserve this
runner's existing proof rather than duplicating it.

## Expanded retrospective acceptance before the next repair

The original #143 acceptance commit `6e9fdf6` remains in history. This extension
is **retrospective coverage of the existing historical app**, independently
authored before any next feature repair. It is not a claim that all existing
production was preceded by these tests. Current product authority is
`docs/zoo-product-experience.md` on default `65d3d68`, #142's selected living
habitat, #140, architecture and the existing #81 viewing model. No tracked Atlas
exists in this Zoo baseline. Mechanical integration of current main into #143
created `da6e1078102c9e9cad223f141b87aa9eac4fb066`; that is the source baseline
without this extension. Live resolver passed with no diagnostics, sourceRevision
`46d8793bb3034326561f876dcc67dbaa5aa1e432`; all25 listed files were previously
fully read at that same revision and matched the already-read stack exactly.
Current AGENTS/CLAUDE, architecture and product decisions were read again.

| Protected proof | Actual authority and observable result |
| --- | --- |
| Individual core behavior | `crates/zoo-core/tests/living_individuals.rs` uses existing ZooGame constructor, real habitat/path/keeper/feed/shelter/adoption commands and real ticks. Stable individual IDs/names survive another adoption/ticks. Walking/Feeding/Resting have actual core targets and deterministic state. The manual Capybara fixture is technical coverage, not a chosen/approved game art asset or a replacement for scenario91. |
| Opening living scenario | Existing visible scenario selection; immutable source identity/seed; camera frames actual first objective; complete actual WASM snapshot; visible animals, themed habitat and valid guest frontage. First objective metadata must equal its actual core row. |
| Actual rendered correspondence | Read-only observation includes `renderer: {revision, instances}` taken from the actual pinned shared renderer submission. Each row carries nodeId/entityKind/entityId/position/assetRef/visible/clipName or guest lookTarget. Animal world positions used for picking/follow exactly match submitted positions; individual name/activity/target match core, allowing enum wire spelling translation only. Guests match actual IDs/states, remain on actual paths, and view from real visible frontage without exceeding capacity; submitted look targets face their viewed habitat. |
| Moving follow and override | Select an actual walking individual through real input while unpaused; sample8 actual ticks with submitted positions/camera targets and capture frames. Both individual and camera move together; ID remains selected. Manual desktop orbit or phone pan releases follow. These are numerical movement/correspondence tests, not an invented FPS, easing duration or animal speed requirement. |
| Objective and construction | Actual first-objective progress advances after a real player action, using watch if that objective is observation or its visible compact action otherwise. Opening category controls exposes a usable item on desktop/phone while the rendered park stays substantially unobscured. Panel navigation cannot mutate simulation. |
| Guest/activity sequence | Capture real authoritative states and rendered frames until Walking/Feeding/Resting plus a same guest's approach→valid Viewing→visible actual thought/response are witnessed. Later checks do not accept initial guest counts, static primitives, formula motion, fake clocks or fabricated success fields as a sequence. Fixture observation loops are finite test safeguards, not new care cadence decisions. |
| Runtime lineage and failure | Actual immutable scenario/core/resource requests must precede test fetches. Fetch source, processing receipt and runtime resources and hash actual response bytes; correlate resource with submitted actor/clip. Fail a genuinely used animal resource through CDP network interception on each viewport; require specific visible asset error, witnessed failed request and no false loaded animal/placeholder fallback. Fault injection cannot provide a successful asset or modify game outcomes. |
| Camera/cosmetic parity | `apps/web/scripts/living-wasm-replay.mjs` initializes real generated Rust WASM, requires its bytes equal the actual production resource, and forwards the original immutable scenario plus recorded actual public gameplay/tick calls. At every actual browser sample the camera-free WASM snapshot must equal the complete browser core snapshot. It contains no local rules, fabricated output or alternate clock for visual evidence. |

### Read-only seam and source audit

Extend the original `livingHabitat()` observation only in a **separate production
context**, not in acceptance authoring. Additional routine wire proposals:
`simulation.objectives` contains the exact current objective rows; individual
rows have actual core `name`, `activity`, `activity_target`; submitted renderer
metadata is described above. Asset rows retain original loaded/provenance/placeholder
and add id/kind/clipNames plus sourceUrl/sourceSha256,
processingReceiptUrl/processingReceiptSha256, resourceUrl/resourceSha256. Scenario
includes immutable sourceUrl/sourceSha256. `replay` records original scenarioJson,
coreWasmUrl/coreWasmSha256 and append-only `commands: [{method,args,result?}]`
observed at the actual Rust/WASM call boundary. Never infer calls from later
snapshots or fabricate equivalent game results. The proposed
`ZooGame.from_scenario_json` name forwards the scenario91 authority; independently
translate its routine signature if the established public loader differs. It
does not authorize a second scenario parser, executor or saved-state injector.
Scenario id/kind/seed must agree with `simulation.scenario`; observation `tick`
is actual absolute game minute `(day-1)*1440 + minute_of_day`, using the current
core time authority rather than a browser frame counter or substituted clock.

New semantic selectors are `[data-build-category]`, `[data-build-item]`,
`[data-action="close-build-category"]`, `[data-objective-action]` when needed and
`[data-asset-error]`; these are test hooks, not prescribed UI wording. All original
scenario/pause/resume/watch/release/objective selectors and earlier coverage remain.

Hash checks establish actual byte identity, **not** expressive art approval or
canonical processing by themselves. Before claiming the asset tier, independently
inspect source/license, invoke asset-tooling's real applicable source/processing/
inspection/receipt verification contract, inspect the actual shared-renderer
resource and animation sampling calls, and correlate captured runtime clips and
frames. A self-advertised receipt, resource hash, clip list or metadata row cannot
replace this audit. Source changes must reach the derivative through the existing
pipeline. Current Zoo asset files/catalog/generated mark are candidates/technical
proof only; #112/#73/#74/#75 and asset-tooling#128 remain essential unresolved
upstream coverage. Do not create substitute art, a local importer/animation engine
or another renderer. Record unavailable actual processing/approval as a blocked
acceptance tier; owner142 already answered the desired direction.

### Narrow original expectation correction

The old partial test required full paused snapshot equality even after individual
watch selection. Current product lines22/30/40 and #142 sequence5 explicitly permit
the first objective to be Observe an animal and require core-backed progress.
Root independently approved this translation before production: a **real watch
command** may only advance `progress`/`completed` on the **same** objective row;
identity, requirements, every other objective and every other snapshot field stay
exact. No objective regression or blanket exclusion is allowed. All camera-only
orbit/follow/release/cosmetics preserve the complete snapshot. Replay forwards the
same legitimate observation command on both sides and varies only presentation.
This corrects a partial historical expectation; it is not implementation-authored
acceptance weakening or a new owner decision.

### Actual baseline and remaining gates

Before extending expectations, on integrated baseline `da6e107`:

- Full `make verify` **PASS**, including existing Rust/core/scene contracts, real
  WASM, strict Clippy/Oxlint/typecheck and production dist build. Log
  `/tmp/zoo143-before-extension-verify.log`.
- Original real desktop/phone CDP orbit/touch/picking/construction/invalid-placement
  **PASS** using actual Chrome and production preview4183. Log
  `/tmp/zoo143-old-protected-default-chrome.log`. Original living journey **RED**
  at missing read-only `livingHabitat()` while real renderer was ready with2493
  nodes. Log `/tmp/zoo143-original-living-baseline.log`. Original normal/failure
  frames and report are retained in
  `/home/moenarch/.local/share/coding-agent/zoo143-before-extension-proof/`.
- New native3 tests compile/run: **1 characterization GREEN,2 RED** at missing
  actual core individual name and activity. Dependent stability/target/all-activity
  assertions after those first failures remain **UNRUN**. Log
  `/tmp/zoo143-expanded-native-baseline.log`. The fixture's initial guest enum
  spelling error was corrected to actual `viewing` before this recorded result.
- Direct initialization of actual generated WASM for replay **RED** at missing
  scenario loader. No scenario creation/replay result is fabricated; downstream
  parity remains **UNRUN**. Log `/tmp/zoo143-actual-wasm-loader-baseline.log`.

After extension, Rust formatting, strict full workspace Clippy and both new `.mjs`
syntax checks **PASS**. Reused public contracts pass unchanged: content baseline1,
content contract15, routing14, viewing-capacity1, visitor-engagement1 (32 total),
plus real generated-WASM catalog/numeric/wrong-leaf acceptance **PASS**. Current
actual production dist core resource and replay binding bytes both hash to
`95f431307016441d634e4ae78804862c9b82edb43246e07834a2fbdbc77390b1`.
All three original CDP default/valid-catalog/invalid-catalog modes were exercised
against this same rebuilt dist and **PASS**; the living mode remains **RED** at
missing hook, later assertions unrun. Logs `/tmp/zoo143-reused-protected.log`,
`/tmp/zoo143-real-content-wasm.log`, `/tmp/zoo143-new-contract-clippy.log`,
`/tmp/zoo143-preserved-valid-catalog.log`, `/tmp/zoo143-preserved-invalid-catalog.log`,
`/tmp/zoo143-expanded-living-baseline.log`. These are production-preview byte
checks, **not** a new published revision or approved art evidence.

Run `make format`, script `node --check`, strict full workspace Clippy, the new
native file, unchanged `content_contract`/`content_baseline`, `routing_regressions`,
`viewing_capacity`, `visitor_engagement`, real `check-content-wasm.ts`, and all
three existing CDP catalog/default modes. Run the expanded living mode above on
the actual production dist. The expanded living baseline stops at the same missing
hook: later follow/objective/guest/art/fault/replay checks are **UNRUN**, not green
or independently witnessed failures. Do not claim complete `make verify` after
adding intentionally RED native tests. Full applicable suites and required hosted
Verify/Foundation audits remain merge gates because there is no verified impact
map across the core/WASM/scene/UI/asset boundaries.

The replay metadata and equivalent opening→watch→activity→construction/fault frame
sequence remain in the existing CI artifact upload; no competing harness or fake
video recording. Actual owner review of desktop/phone normal/focused frames for
expressive recognizability, readable miniature world/themed exhibit/visitor reaction
and compact UI remains **pending**. Functional correctness, pipeline validation,
owner visual approval and actual Pages publication are distinct gates. Keep #143
draft/RED; do not close #140/#142 or their prerequisite issues on technical tests
alone, and never present candidate geometry as approved artwork.
