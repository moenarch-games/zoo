# Zoo architecture

Zoo is a game first, but it must not become a second implementation of generic engine infrastructure that already has an authority elsewhere in this workspace.

The architectural goal is the smallest playable vertical slice built on the intended long-term foundations, not the smallest self-contained amount of code.

## Authority map

| Concern | Authority | Zoo responsibility |
| --- | --- | --- |
| Park rules, money, placement validity, guest/animal decisions, staff work, saves | `zoo-core` | Own the deterministic game semantics and stable game IDs. |
| Zoo-specific scene framing, orbit/zoom policy, mapping game-space state toward shared scene contracts | `zoo-scene` | Adapt Zoo state without becoming a generic renderer or camera-math authority. |
| Camera matrices, transforms, renderer-independent mesh/asset semantics, animation primitives, LOD, and the reusable concrete Three.js browser renderer | `moritzbrantner/3d-lab` | Supply Zoo scene data and interaction policy; consume the shared renderer instead of constructing a parallel renderer. |
| Generated/processed asset provenance, canonical asset catalog, reproducible 3D processing | `moritzbrantner/asset-tooling` | Declare Zoo asset intent and consume verified outputs. |
| Collision response, rigid-body motion, CCD, physical spatial queries | `moritzbrantner/physics-engine` | Integrate only where physical truth is part of gameplay; do not approximate engine behavior in React. |
| ECS storage experiments and benchmark evidence | `moritzbrantner/ecs-lab` | No direct runtime dependency. Reusable ECS functionality must first become an intentional shared foundation rather than being copied out of the lab. |
| Geographic/MapLibre map presentation and geo-data utilities | `moritzbrantner/maps` | No forced dependency for the current tile park. Reuse only a genuinely applicable data/spatial contract; Zoo owns its park-grid semantics. |
| React UI, touch/mouse/keyboard interaction, HUD/windows, and thin consumer glue | `apps/web` | Present authoritative state and translate user intent into commands. It must not own game balance, placement validity, generic rendering, camera projection math, or physics semantics. |

## Current correction

The browser presentation grew from an MVP into a local pseudo-3D engine: CSS perspective/3D transforms, local yaw/pitch projection policy, object billboard compensation, and a large `App.tsx` all accumulated in the product repository. That proved the game loop but created the wrong durable authority boundary.

The migration therefore proceeds upstream-first:

1. The missing orthographic/isometric-capable camera primitive was added to `3d-lab` rather than extending Zoo's CSS camera math.
2. `zoo-scene` is the thin Zoo-owned adapter over one exact pinned `3d-lab` revision. It owns park framing, free orbit, pitch bounds, zoom limits, pan clamping, and the mapping from a picked ground point to the park grid, while returning shared `three-d-camera` perspective matrices and unprojecting through shared `three-d-projective` inversion.
3. `3d-lab` now also owns the reusable concrete Three.js renderer. Zoo consumes it at the same exact accepted revision; React does not instantiate its own generic renderer.
4. The park view is one shared-renderer canvas. Terrain, fences, buildings, concessions, habitat furnishings, animals, guests, staff, litter, maintenance markers, and interaction overlays (hover, placement ghosts, selection) are all shared scene nodes composed by `apps/web/src/park-scene.ts`. The earlier transitional DOM tile/object layer, its MutationObserver reprojection, and the CSS pseudo-3D stylesheets have been removed. Static nodes (terrain, fences, buildings, and the surrounding scenery) are submitted as shared-renderer instance batches, grouped per unit primitive and 16-tile ground cell by `apps/web/src/scene-batching.ts`, so the park costs roughly a hundred draw calls instead of one per part; animated actors remain individual nodes.
5. Picking is renderer-side: ground/tile picks come from `zoo-scene` (shared inverse view-projection intersected with the park ground plane), and actor/building picks compare pointer position against anchors projected with the shared `createWorldProjector` helper. Final command validity remains a `zoo-core` decision.
6. Durable 3D assets move through `asset-tooling`; renderer-independent mesh/material/LOD semantics remain in `3d-lab`.
7. Path construction, habitat ownership, guest choices, welfare, economy, staff tasks, and other game-specific rules remain in `zoo-core`.
8. `physics-engine` is added only for interactions whose gameplay semantics require physical collision/motion truth. Tile occupancy and ordinary park pathfinding do not become physics problems merely because the game is rendered in 3D.

## Construction transactions

Construction is planned and committed in `zoo-core` (`crates/zoo-core/src/construction.rs`, #92). `evaluate_path_stroke`, `evaluate_concession`, `evaluate_habitat_rect` and `evaluate_demolition` return the exact tiles, price, affected objects, blocking tiles and rejection reason; the matching commands plan again against the current park, so a stale preview never spends or builds. One gesture is one all-or-nothing transaction: a path stroke is sent whole on release and Rust joins its points into a 4-connected line.

Undo/redo reverses construction only, through validated inverses: at most 16 transactions; undo refunds exactly what the transaction spent and redo charges it again with the same stable IDs; undo is refused once the construction has been used (guests or staff on a path, litter, sales, maintenance, animals, shelters, care work), leaving ordinary demolition. Demolition is free and is undoable only when it destroyed nothing irreversible. History ends with the in-game day so refunds always land in the ledger that holds the spending, is never saved (a load starts empty), and is replayable because undo and redo are recorded session commands. The browser only gathers gesture tiles, shows Rust's preview, and cancels on Escape, pointer cancel, lost pointer capture or a second finger.

## Management alerts and overlays

Park problems are `zoo-core` state (`crates/zoo-core/src/alerts.rs`, #94). After every command and every simulated minute Rust re-reads each alert condition (empty or low feed store, cut-off service gate, unstaffed habitat, low welfare, unsuitable group, destination guests cannot reach, broken-down or unserviced stand, crowded viewpoint, litter backlog, keeper without a zone) and keeps one alert per reason code and object, with severity, the tile of the cause, a suggested action and its first and last occurrence; a resolved condition or a removed object drops its alert. Alerts are saved (schema 4), so replay and reload agree. Overlays and the daily summary (`crates/zoo-core/src/overlay.rs`) are read-only views computed on request: `overlay_json(kind)` grades tiles for access, welfare, staff coverage, cleanliness, facilities or guest viewing and lists the problem objects, and the snapshot's `summary` carries today's numbers with the entities and ledger lines behind each. The browser re-reads the active overlay with each snapshot, never per frame, and only sorts, filters and lays out these values in the management workspace (`apps/web/src/ManagementWorkspace.tsx`).

A habitat is the row-major set of tiles one closed fence encloses (#95, `shape.rs`, `enclosure.rs`; save schema 5 converts older rectangles). The outline tool sends grid corners (corner `(x, y)` is the north-west corner of tile `(x, y)`, picked by `zoo-scene`); Rust joins them into unit fence edges, requires one closed loop that never revisits a corner, and derives the interior by row parity, so drawing direction and start never matter and holes, pinches and self-crossings cannot exist. Rectangles are a convenience over the same representation. `evaluate_habitat_outline`/`place_habitat_outline` build a new enclosure; `evaluate_habitat_reshape`/`reshape_habitat` move an existing habitat's fence while keeping its ID, animals, care, staff and gate (re-picked only when its segment leaves the fence). A reshape charges only for added tiles and new fence segments, refunds nothing it removes, rejects fences that hold fewer animals than live there or that a working keeper stands inside, and is undoable while the old fence still fits. Viewing spots, animal roaming and gate segments follow the enclosure's fence clockwise from its first tile. Every habitat is fenced with a content-defined fence type (`docs/content/catalog.md`): new habitats get the first one, reshapes price new segments at the habitat's own type, upkeep is per segment of that type, species with a higher minimum fence height cannot be adopted behind it, and `evaluate_fence_upgrade`/`upgrade_fence` rebuild the whole fence as another type at full price (save schema 6 moves the old implicit fence into content as the wooden rail).

## Dependency policy

Cross-repository dependencies must be pinned to exact accepted revisions. If a required shared contract is unavailable or incompatible, validation should fail closed rather than silently falling back to a second local implementation.

Both `zoo-scene` and the browser renderer consumer pin the accepted `3d-lab` revision. A pin moves only to another reviewed/accepted revision; it must not become a branch-tip dependency.

A local substitute is acceptable only when all of the following hold:

- the behavior is genuinely Zoo-specific, or the shared authority explicitly does not own it;
- the substitute does not establish a competing generic contract;
- the boundary and later migration cost are documented;
- deterministic/game-authoritative behavior remains in Rust.

## Rendering boundary

`zoo-core` emits stable game state and game-space coordinates. `zoo-scene` maps the presentation-facing parts into shared camera/scene semantics. `@moritzbrantner/three-d-renderer` from `3d-lab` owns concrete Three.js scene/GPU adaptation, including the WebGPU-camera to WebGL-depth boundary and reusable world-to-screen projection.

Camera orbit, zoom gestures, framing targets, and persistence are Zoo interaction policy. Projection/view math and concrete rendering are shared 3D infrastructure. Selection and placement intent may originate in the presentation layer, but final validity remains a `zoo-core` decision.

The park has no DOM interaction layer. `apps/web/src/ParkView.tsx` interprets mouse, wheel, keyboard, and touch gestures (one-finger pan, pinch zoom, twist rotate, two-finger vertical tilt) into `zoo-scene` camera calls and tile/actor picks, and translates tool gestures into the same `zoo-core` commands on desktop and phone.

Each park-view mount owns exactly one shared renderer. Unmounting disposes it and force-loses its WebGL context, so repeated load/dispose cannot accumulate contexts or GPU resources; `apps/web/scripts/renderer-lifetime-proof.mjs` remounts the view repeatedly and checks that live resources return to baseline. A catalog that fails the `zoo-core` content contract stops at a "Park could not start" screen with diagnostics export rather than booting a partial park.

Mobile and desktop inputs must converge on the same commands. Touch-specific gesture handling is presentation state and must not create a second rules path.

## Asset boundary

Zoo asset specifications and accepted generated outputs carry provenance through `asset-tooling`. Mesh/material/LOD processing remains owned by the corresponding `3d-lab` contracts and processors. Zoo may choose art direction and species/building requirements; it should not implement another generic asset pipeline.

The first asset-tooling asset at runtime is the scenery (#59 S2, #86): the rolling `zoo-scenery` release (`tree.mesh` round/pine/acacia, `bush.mesh` round, `grass.mesh` tuft, `rock.mesh` round/boulder, `static-glb-png-v1`). It surrounds the park and furnishes habitats with seeded, per-habitat-capped rocks, a boulder, grass tufts and the savanna acacia; these placements are cosmetic and never reach zoo-core. `assets/scenery/` pins the release tag, the tar SHA-256, the asset-tooling commit, Zoo's own copy of `export.ref.json` and the release lineage. `assets/effects/` pins the rolling `zoo-effects` release (dust/leaf/splash particle flipbooks) the same way, plus Zoo's copy of its descriptor (atlas layouts and burst presets). `bun run assets` (part of `build:web` and `dev`, `scripts/stage-asset-releases.ts`) downloads each tar, checks the pin and runs asset-tooling's `pack-asset-bundle.ts --verify --ref` at the pinned commit before staging the files and a provenance manifest into `public/zoo-scenery/` and `public/zoo-effects/`. The park view lowers the meshes with `three-d-renderer/static-glb`, requires each adapted resource key to equal the staged SHA-256, and fails closed otherwise; the Park manager panel shows both lineages under *Scenery credits*. The particle effects are cosmetic and optional: `park-effects.ts` turns presentation cues (a keeper `working` in a habitat → dust, a keeper or guest passing foliage → leaves, an animal stepping into a pond → splash) into bounded, cosmetically seeded `frame.effects.bursts` on the view's seconds clock, declares an atlas only while one of its bursts is live, sends `enabled: false` under `prefers-reduced-motion`, and never feeds game RNG or zoo-core. Moving to a new release means updating the pin, the `export.ref.json` copy and the lineage together; both pins and the Verify and Pages asset-tooling checkouts use the same commit.

## Physics boundary

The current management simulation does not need rigid-body physics for ordinary grid movement, placement, or guest pathfinding. When gameplay introduces physically meaningful interactions, Zoo should adapt stable Zoo entity IDs/state to `physics-engine` bodies and consume deterministic results back into the game simulation. React must never become the collision authority.

## ECS boundary

`ecs-lab` is evidence and experimentation, not the application runtime. If Zoo reaches a scale where an ECS materially improves simulation organization or performance, the reusable ECS contract should first be extracted into an explicit shared crate/repository, then adopted by Zoo. Copying a lab implementation directly into `zoo-core` would recreate the same ownership problem this architecture is intended to prevent.
