# Zoo Decision Atlas

This is the canonical hierarchical record of **owner-approved product direction**, seeded from the [ten agreed product choices](../zoo-product-experience.md), [Zoo architecture](../architecture.md) and [roadmap #20](https://github.com/moritzbrantner/zoo/issues/20). Topic headings are navigation; semantic leaves are exactly **Decisions** or **Questions**. Preserve each dotted ID. Do not treat an existing implementation as owner approval for a new product requirement.

## Binary implementation evidence

Nested `- [x]` items identify **only the precisely cited implemented/tested capability**; `- [ ]` identifies the specific missing or unverified remainder. No other progress states. A checked technical foundation **does not imply full decision/product acceptance**, and a Decision without a checklist is *unaudited*, not 0%. An issue is temporary execution state; the Atlas is permanent product intent. Use repository-relative `path::symbol` plus tests/immutable evidence instead of fragile source line numbers. The current `daq` parser does not yet index nested checkboxes; see [coding-tooling #322](https://github.com/moritzbrantner/coding-tooling/issues/322) and [#323](https://github.com/moritzbrantner/coding-tooling/issues/323).

- **1** First-minute product experience
  - **1.1** Art direction
    - **1.1.1** [decision] The Zoo should be a readable miniature 3D management world, using expressive cartoon animals and themed habitats, with the playable park as the dominant surface. ([product decision 1/6/7](../zoo-product-experience.md), [#112](https://github.com/moritzbrantner/zoo/issues/112))
      - [x] A real rendered park and Zoo-specific scene adapter already exist: `apps/web/src/ParkView.tsx::ParkView` and `apps/web/src/park-scene.ts`, driven by real `zoo-core` snapshots. The existing production-browser proof is `apps/web/scripts/capture-park-view-proof.mjs`.
      - [ ] Source-backed expressive animal animation, coherent themed opening habitat and owner-approved desktop/phone visuals are not implemented/verified by the existing park renderer alone ([#112](https://github.com/moritzbrantner/zoo/issues/112), [#142](https://github.com/moritzbrantner/zoo/issues/142)).
  - **1.2** Initial scenarios and objectives
    - **1.2.1** [decision] Support empty, partially prepared and already living initial parks in one game, with a compact authoritative objective checklist and initial camera framing the first objective. ([product decisions 2–5](../zoo-product-experience.md), [#91](https://github.com/moritzbrantner/zoo/issues/91))
      - [x] Versioned species and facility catalog loading is implemented through `crates/zoo-core/src/content.rs::Catalog::parse`, `crates/zoo-core/content/catalog.json`; independent native tests `crates/zoo-core/tests/content_contract.rs` and WASM/browser checks `apps/web/scripts/check-content-wasm.ts` are recorded in merged [PR #144](https://github.com/moritzbrantner/zoo/pull/144).
      - [ ] Authoritative scenario selection, three agreed starting-park variants, first-objective framing and objective checklist remain to be implemented and accepted ([#91](https://github.com/moritzbrantner/zoo/issues/91), [#93](https://github.com/moritzbrantner/zoo/issues/93), [#142](https://github.com/moritzbrantner/zoo/issues/142)).
  - **1.3** Watching real animals
    - **1.3.1** [decision] Selecting **one individual** animal should smoothly watch/follow its real position, display its name and current activity with compact UI, and allow returning to ordinary park navigation. ([product decision 9](../zoo-product-experience.md), [#141](https://github.com/moritzbrantner/zoo/issues/141))
      - [x] Existing park scene/pointer selection and orbit/touch navigation are handled by `apps/web/src/ParkView.tsx::ParkView` and exercised by `apps/web/scripts/capture-park-view-proof.mjs`.
      - [ ] Existing animal picking identifies a habitat rather than a stable individual with a live name/activity; real per-animal follow and cancellation acceptance remain missing ([#96](https://github.com/moritzbrantner/zoo/issues/96), [#141](https://github.com/moritzbrantner/zoo/issues/141)).
- **2** Simulation ownership
  - **2.1** One authoritative park model
    - **2.1.1** [decision] Zoo game rules, catalog identity, guest viewing, animal activity and economics belong to deterministic `zoo-core`; rendering responds to authoritative outcomes, not client-fabricated gameplay. ([architecture](../architecture.md), [#90](https://github.com/moritzbrantner/zoo/issues/90))
      - [x] The Rust `crates/zoo-core/src/lib.rs::ZooGame` and versioned catalog in `crates/zoo-core/src/content.rs` are implemented; `crates/zoo-core/tests/content_contract.rs` verifies deterministic catalog-driven behavior (merged [PR #144](https://github.com/moritzbrantner/zoo/pull/144)).
      - [x] Guest-path/viewing rules have deterministic Rust regression tests in `crates/zoo-core/tests/routing_regressions.rs` (viewing-route selection and path connectivity).
      - [ ] Stable *individual* animal activities (walking, feeding and resting) integrated with real mesh animation are not proven by aggregate habitat welfare or existing cosmetic movement ([#96](https://github.com/moritzbrantner/zoo/issues/96), [#107](https://github.com/moritzbrantner/zoo/issues/107)).
- **3** First owner-visible acceptance
  - **3.1** Living habitat on GitHub Pages
    - **3.1.1** [decision] The first observable milestone on the actual Zoo Pages game is a **populated living themed habitat**, purposeful animals, guest reactions and click-to-follow one animal—not a backend test or a static asset viewer. ([product decision 10](../zoo-product-experience.md), [#142](https://github.com/moritzbrantner/zoo/issues/142))
      - [x] The existing browser verification has a real production-built park interaction journey, including camera, touch and construction rejection checks: `apps/web/scripts/capture-park-view-proof.mjs`; existing baseline passes are recorded in [#140](https://github.com/moritzbrantner/zoo/issues/140).
      - [ ] Independent living-habitat acceptance is deliberately **red** in draft [PR #143](https://github.com/moritzbrantner/zoo/pull/143); the full real animal/guest/watch scenario has not yet passed or been visually approved ([#140](https://github.com/moritzbrantner/zoo/issues/140), [#142](https://github.com/moritzbrantner/zoo/issues/142)).

## Execution handoffs

One active implementation worker per repository, with sequential acceptance and implementation contexts. Keep the GitHub issue queue mostly flat: deliver and verify one bounded slice, then close an oversized issue as superseded with links to small successor issues for every unfulfilled requirement. Do not close the product Decision/check its whole experience from a merged technical prerequisite.
