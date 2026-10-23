# Zoo

A browser-first zoo management game with a free-camera 3D park inspired by classic management sims.

The project starts from a playable park loop, but it no longer treats "MVP" as permission to create parallel engine infrastructure inside Zoo. New vertical slices should use the intended shared foundations wherever they are already authoritative.

## Architecture

- **Rust (`crates/zoo-core`)** — deterministic Zoo simulation, placement, economy, guest movement, animal state, staff work, and save semantics.
- **`moritzbrantner/3d-lab`** — reusable renderer-independent camera, transform, mesh/asset, animation, and LOD semantics. Zoo owns game-specific scene composition and camera interaction policy, not generic 3D math.
- **`moritzbrantner/asset-tooling`** — reproducible generated/processed asset provenance and canonical asset operations used by Zoo assets.
- **`moritzbrantner/physics-engine`** — collision/rigid-body authority when future Zoo gameplay actually requires physical simulation. Ordinary tile placement/pathfinding stays game-specific.
- **WebAssembly** — thin browser boundaries around authoritative Rust behavior.
- **React + TypeScript (`apps/web`)** — HUD, management windows, touch/mouse/keyboard input, and the concrete renderer adapter.
- **No direct ECS Lab dependency** — `ecs-lab` remains an experiment harness; reusable ECS functionality must first become an intentional shared foundation.
- **No forced Maps dependency** — the current park is a game-space tile world, not a geographic MapLibre surface. Reuse `maps` only where a real shared spatial contract applies.

The park renders as one shared `3d-lab` Three.js canvas. Camera framing, orbit/tilt/zoom limits, and ground picking live in `zoo-scene` over `3d-lab` camera and projective math; there is no DOM or CSS game-object layer. See [`docs/architecture.md`](docs/architecture.md).

The simulation core owns the game rules. Presentation code does not duplicate placement, economy, camera projection, or physics logic.

## Run

Requirements: stable Rust with the `wasm32-unknown-unknown` target, `wasm-pack`, and the latest stable Bun.

```sh
make install
cd apps/web
bun run dev
```

## Current playable loop

1. Extend the entrance path.
2. Place a habitat next to a path.
3. Inspect the habitat.
4. Adopt animals.
5. Explore the park: drag to pan, right-drag (or two-finger twist / two-finger vertical drag) to rotate and tilt, scroll or pinch to zoom. Keyboard: WASD/arrows pan, Q/E rotate, R/F tilt, +/− zoom, Home resets.
6. Run the clock and watch guests, staff, animals, and operations react.

The product roadmap starts at GitHub issue #20. Architecture convergence is part of the roadmap rather than deferred generalized-engine work.

## Development checks

Run `make verify` for the repository gate. It installs the committed Bun graph in frozen mode, builds both WASM adapters once, and checks Rust formatting, strict Clippy, workspace tests, web formatting, type-aware linting, strict TypeScript, and the production build. Cargo commands use the committed lockfile.

Use `make format` to write formatting changes; verification never fixes source or rewrites lockfiles. `make format-check`, `make lint`, and `make typecheck` provide focused checks; typechecking requires `make wasm` after a clean checkout. Shared coding-tooling uses the same Bun package scripts through `.coding-tooling.json`.

Engineering policy is resolved from the live shared `coding-agent-conventions` authority as described in `AGENTS.md`. Zoo retains its game and foundation authority boundaries. Generated bindings, dependencies, builds, browser captures, and agent run state remain ignored local output.
