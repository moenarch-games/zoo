# Zoo product experience: first minute and visual identity

**Product-intent authority:** The [Decision Atlas](decisions/atlas.md) holds the durable owner-approved choices. This document preserves the detailed 2026-10-11 ten-question design source and owner-verifiable acceptance as supporting specification; it is not a second independent decision database, nor evidence that the deployed site implements the experience. The [1.0 roadmap](https://github.com/moenarch-games/zoo/issues/20) remains the authority for broader scope and sequencing, and [architecture](architecture.md) remains the authority for component ownership.

## Ten settled decisions

| # | Topic | Approved product direction |
| --- | --- | --- |
| 1 | Art direction | **Readable miniature world**, inspired by Parkitect's legibility: colorful, simplified genuine 3D, crisp silhouettes and a clear management-game hierarchy; no copied assets or UI. |
| 2 | Starting parks | **Multiple scenarios**, including an entirely empty plot, a partially prepared park and an already living/operating starter zoo. These are data-defined starting states within one game, not separate engines. |
| 3 | Guidance | **Compact visible objective checklist** in goal-driven scenarios; progress updates from authoritative game state. The player can still act freely; sandbox is not forced into a scripted objective sequence. |
| 4 | Starting camera | **Frame the first scenario objective** when the scenario opens, then allow immediate normal camera interaction. Do not require a cinematic flyover or start with an unhelpful generic park overview. |
| 5 | Construction | **Compact bottom toolbar** with small, focused category panels; leave the playable park unobscured. Retain keyboard and touch support. |
| 6 | Animals | **Expressive cartoon characters**, with recognizable species features, larger expressive faces and playful readable animation, rather than naturalistic or interchangeable blocky toy shapes. |
| 7 | Habitats | **Themed exhibits**: coherent miniature landscapes, planting, buildings, viewing areas and decorations. Preserve actual welfare/placement semantics; decorative assets do not silently change simulation rules. |
| 8 | First impression | **Living animals first** wherever a scenario starts populated: visible walking, eating, resting and guest reactions should make the scene feel alive before opening management windows. An empty scenario naturally becomes alive only as the player builds it. |
| 9 | Click an animal | **Watch/follow first**: selecting an *individual* animal smoothly focuses the camera and displays its name and current activity with minimal UI. Detailed care/management remains reachable through a separate deliberate action; no large management window should obscure the initial watching experience. |
| 10 | First GitHub Pages proof | **Living habitat**: load an attractive populated zoo, show expressive animals walking/eating/resting, let the user click one to follow it, and show visitors actually watching/responding. The *build a habitat* and *care-and-improve* demonstrations are later scenarios, not substitutes for this first visual acceptance. |

## First owner-verifiable GitHub Pages slice

**Entry:** From a clean hosted/browser session, the player can open the living-zoo scenario without constructing a habitat first. The first camera framing presents the relevant habitat and visible animals (the opening scenario objective can be to observe an animal). The main screen is an interactive rendered park, not a text tutorial, static marketing screenshot, or detached asset viewer.

**Walkthrough:** 
1. Open the published build or the exact production-built Pages preview and select/start the living-zoo fixture. The camera frames the first objective and an unobtrusive checklist indicates the next action.
2. See an appealing miniature park with at least one convincing themed animal exhibit, recognizable and **expressive cartoon animals** in it, plus visitors who traverse and view from valid paths.
3. Observe **actual activity** progress between walking, feeding/eating and resting through the authoritative animal state as applicable. Animals must not merely loop one arbitrary pose disconnected from their state.
4. Select an **individual** animal in the 3D park; camera smoothly focuses or follows that animal, with a compact name/activity indication. Other animals in the same habitat must remain distinguishable targets. The player can return to ordinary park navigation without rebuilding/altering simulation state.
5. Watch guests approach a valid viewing point, look at animals and respond visibly to the viewing situation; preserve the accepted visibility/capacity semantics from issue #81.
6. Open the compact construction category controls without losing meaningful visibility of the park. The shown objective state reflects actual progress, not browser-only made-up game rules.

**Art/reference review:** Owner-selected inspiration came from third-party game images during the session, **not actual Zoo screenshots or licensed source assets**. A final visual approval requires screenshots/video from the actual committed Zoo build, at normal park zoom and closer watch zoom, on desktop and phone. Visual acceptance should judge readability, coherent miniature-world art, recognizable expressive animals, visible activities, themed exhibits, uncluttered toolbar/objectives and visitors reacting. This approval is distinct from automated correctness tests.

**Automated browser acceptance (authored independently before feature implementation):**
- Extend the existing real-Chromium/CDP journey at `apps/web/scripts/capture-park-view-proof.mjs` and `.github/workflows/verify.yml` rather than introducing a competing browser harness. If replacing it with Playwright later, migrate the existing coverage rather than duplicating it.
- Drive the **production web build** and the **real Zoo WASM game**, using an authored deterministic living-zoo scenario/fixture. Do not stub the Rust simulation or supply fake DOM animals/guest counts to make the test pass.
- Assert scenario selection/initial camera target, a visible objective backed by authoritative state, independently pickable animal identity (not just habitat ID), focus/follow state and a visible individual name/activity, valid visitor viewing state, and recovery of normal camera controls. Freeze/synchronize declared game state for reproducibility; do not rely solely on screenshot pixels or brittle wall-clock sleeps.
- Capture an opening park screenshot, a focused-animal screenshot and an interaction recording or equivalent visual proof; include desktop and phone viewports. Assert the park remains visible and control targets are usable at both sizes. Attach snapshots/replay metadata to CI.
- Demonstrate at least two distinguishable animal activities (including a feeding/resting sequence when supported by the fixture) and visible guest presence in the actual scene. Test missing assets/load failures openly rather than falling back silently to unrelated placeholder primitives.
- Assert neither camera-follow interpolation nor cosmetic animation changes `zoo-core` rules, guest viewing/crowding, animal welfare or pathfinding; representative camera orbit/tilt/picking regressions from the existing dogfood remain green.
- **Do not mark the visual slice complete** on the strength of WASM tests, disconnected asset previews, or a staged screenshot that is not backed by the running game.

**Implementation boundaries:** `zoo-core` owns stable individual animal identity/activity, guest viewing and scenario state; `zoo-scene` owns game-specific camera targeting; `3d-lab` owns reusable camera/render/animation semantics; `asset-tooling` owns source-to-runtime asset processing; the React UI owns toolbar/checklist/HUD and input. Do not add a second renderer, local animation engine or fake client-side animal behavior.

## Existing roadmap and distinct work

- Scenario variants and first-objective framing: #91 (scenario models), #93 (onboarding), #117 (later authored campaign objectives).
- Compact building UI and visuals: #22, #27, #94; themed habitat assets: #86, #100 and launch consistency #119.
- Species identity and watch target: #96; purposeful behavior: #107; expressive asset/animation integration: #112; guest viewing: #81 (accepted semantics).
- General browser scene/camera foundations: #59; complete 1.0 acceptance: #126.

**Not decided in this session:** which exact species begins the demonstration, its named character, color palette, precise first task wording, specific path/habitat coordinates, zoom easing constants, minimum FPS, or the final shape of optional sandbox controls. Do not present those as owner-approved requirements. Use independently authored acceptance fixtures and reasoned implementation defaults where reversible; ask for a decision only when a choice materially changes the product.
