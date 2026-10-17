.PHONY: install test wasm web format format-check lint typecheck verify assets assets-development assets-production assets-validate

install:
	cd apps/web && bun install --frozen-lockfile

test:
	cargo test --workspace --locked

wasm:
	cd apps/web && bun run wasm

web: install wasm
	cd apps/web && bun run build:web

format:
	cargo fmt --all
	cd apps/web && bun run format

format-check:
	cargo fmt --all --check
	cd apps/web && bun run format:check

lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cd apps/web && bun run lint

typecheck:
	cd apps/web && bun run typecheck

# Blender ignores script failures unless --python-exit-code is set.
assets-development:
	blender --background --factory-startup --python-exit-code 1 --python tools/build_assets.py -- --stage development

assets-production:
	blender --background --factory-startup --python-exit-code 1 --python tools/build_assets.py -- --stage production

assets: assets-development assets-production

assets-validate:
	blender --background --factory-startup --python-exit-code 1 --python tools/build_assets.py -- --validate

verify:
	$(MAKE) install
	$(MAKE) format-check
	$(MAKE) wasm
	$(MAKE) lint
	$(MAKE) test
	$(MAKE) typecheck
	cd apps/web && bun run build:web
