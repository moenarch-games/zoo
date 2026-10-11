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
Paused selection/orbit must preserve the full simulation snapshot.

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
