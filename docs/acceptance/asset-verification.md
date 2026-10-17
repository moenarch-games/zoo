# 3D catalog verification acceptance — #75

Authority: issue #75, the existing validator `tools/build_assets.py` (`validate_outputs()`), the protected #73 contract (`docs/acceptance/asset-attachments.md`, `tools/tests/attachment_pose_blender.py`), `assets/3d/README.md`, `AGENTS.md` and `docs/architecture.md`. Baseline is default `f6214541cfee896490b8133ca89c10ec1ebd927f`. This contract was authored in an independent acceptance context before any workflow, Makefile or validator change. Shared conventions resolved without diagnostics at sourceRevision `5ad0db36974c8b0e5f9812c90ee34a4f227852c3`; all 25 resolved files and local AGENTS/CLAUDE were read.

This is verification integration only. It does not decide #74's editable-source authority, does not approve art, and does not move reusable processing or provenance into Zoo.

## Required check

One path-scoped GitHub workflow (the name is the implementer's choice) runs the 3D catalog checks.

- **Trigger.** `pull_request` filtered by `paths:`. The list must include at least `tools/build_assets.py`, `tools/tests/**`, `assets/3d/**` and the workflow file itself. It must also include `Makefile` if the workflow calls `make`. A `push` trigger to `main` is optional, but if present it is path-scoped the same way.
- **Unrelated PRs.** Unrelated PRs pay nothing. `verify.yml`, and every other workflow on an unscoped `pull_request`/`push` trigger, runs no Blender and installs no Blender. `make verify` must not reach Blender. No environment fingerprint, runner-identity receipt or extra validation job is added (ENV-004, GIT-005).
- **Pinned runtime.** Blender `5.0.1`, the identity already verified for #73. The archive is `blender-5.0.1-linux-x64.tar.xz`, checked with `sha256sum` against `8019580ee1b7262e505f4196a00237ccf743c88d205b38d34201510676e60b09`; the binary reports build `a3db93c5b259`. The pin is narrow (REP-008). It exists because the #73 tolerance and importer behaviour were accepted on this build. Downloading the archive is explicit dependency acquisition before the deterministic steps (REP-016).
- **Fail closed.** Every Blender invocation passes `--python-exit-code 1`. Without it, Blender exits `0` when the script raises. This was observed at baseline with the existing Makefile recipe: `make assets-validate` on a copy with a deleted `entrance.bin` printed `AssertionError: missing buffer` and exited **0**. Every Makefile Blender target (`assets-development`, `assets-production`, `assets-validate`) carries the flag; the static invariant checks all of them.
- **No residue.** The check writes only to a runner temp directory. Tracked files stay unchanged, and no `tools/__pycache__` is left behind. At baseline, `attachment_pose_blender.py --generate` writes `tools/__pycache__/build_assets*.pyc` into the checkout, so the workflow sets `PYTHONDONTWRITEBYTECODE=1` or uses an equivalent (REP-014).

The check must run exactly these commands, or equivalents that differ only in output paths:

```sh
BLENDER=/home/moenarch/.local/share/coding-agent/video-84-tooling/blender-5.0.1-linux-x64/blender   # CI: verified 5.0.1 download
export PYTHONDONTWRITEBYTECODE=1
python3 -m py_compile tools/tests/attachment_pose_blender.py tools/tests/asset_validation_negative.py
# 1. Existing structural/importability validator on the tracked catalog.
timeout -k 10s 120s "$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python tools/build_assets.py -- --validate
# 2. #73 attachment contract: all ten animated candidates, saved source + both derivatives.
timeout -k 10s 300s "$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python tools/tests/attachment_pose_blender.py -- --root "$PWD" --output "$TMP/zoo73-attachments" --source
# 3. #73 contract on fresh output of the actual generator, all ten candidates (no --assets subset).
timeout -k 10s 600s "$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python tools/tests/attachment_pose_blender.py -- --root "$PWD" --output "$TMP/zoo73-generated" --generate --source
# 4. #75 negative harness: the unchanged validator rejects each injected defect.
timeout -k 10s 300s "$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python tools/tests/asset_validation_negative.py -- --root "$PWD" --output "$TMP/zoo75-negative"
# 5. Static wiring invariant (plain Python; also cheap to run anywhere).
python3 tools/tests/asset_verification_workflow.py --root "$PWD"
```

The `timeout` bounds only guard completion. They are not performance limits.

## Executable evidence

### `tools/tests/asset_validation_negative.py` (Blender)

For each case, the harness:

1. Copies the tracked development `.gltf`/`.bin`, the production `.glb` files and `catalog.json` into `<output>/cases/<case>/`.
2. Injects exactly one defect into that copy.
3. Points only the validator's path constants (`ROOT`, `ASSETS`, `DEVELOPMENT`, `PRODUCTION`, `PREVIEWS`, `MANIFEST`) at the copy, and asserts that none of them escapes it.
4. Runs the unchanged `validate_outputs()`.

The harness records rules and evidence as follows:

- **What counts as a detection.** Any exception fails closed. A case counts as detected only when the failure is attributed to the asset that was altered: the validator's own `asset` loop variable at the raise must name that asset, and the message must match the defect's diagnostic. An unrelated earlier failure therefore cannot satisfy a case.
- **Positive control.** The unmodified copy must pass. A validator that always fails cannot satisfy the contract.
- **Tracked files.** It hashes `tools/build_assets.py` and `assets/3d/**` before and after the run and fails on any change.
- **Restricted output location.** It refuses an output directory inside the checkout, or one that contains the checkout.
- **Report.** It writes `asset-validation-negative-report.json`, which lists the changed copy paths, the diagnostics, the Blender version and the generator SHA-256.

| Case | Injected defect (copy only) | Required outcome | Baseline |
| --- | --- | --- | --- |
| `unmodified_copy` | none | passes | OK, passed |
| `dev_missing_buffer` | delete `entrance.bin` | `missing buffer` on entrance | OK |
| `dev_truncated_buffer` | drop last 8 bytes of `entrance.bin` (shorter than declared `byteLength`) | buffer-size failure on entrance | OK, detected only by Blender's importer (`buffer is smaller than requested size`); the validator's own buffer check tests file existence only |
| `glb_corrupt_header` | GLB magic → `XXXX` | `invalid GLB` on entrance | OK |
| `glb_truncated` | GLB 64 bytes short of header length | `invalid GLB` on entrance | OK |
| `dev_nonfinite_position` | NaN in first development POSITION float | `non-finite positions` | OK |
| `prod_nonfinite_position` | +Inf in first production POSITION float | `non-finite positions` | OK |
| `catalog_triangle_drift` | catalog `triangles` +1 | `triangle drift: entrance` | OK |
| `over_budget_output` | real elephant outputs (both stages) delivered as `grass_tile`, catalog count consistent | `triangle budget exceeded: grass_tile` | OK |
| `catalog_budget_exceeded` | catalog `triangleBudget` set below observed `triangles` for entrance | budget failure on entrance | **UNEXPECTED: passes**. README says budgets are "declared per asset in `catalog.json` and enforced", but the validator only compares against the in-code `TRIANGLE_BUDGETS` |
| `dev_clip_without_motion` | every development `walk` sampler key frozen to its first key | `animation has no motion …capybara.gltf:walk` | OK |
| `prod_clip_without_motion` | every production `idle` sampler key frozen | `animation has no motion …capybara.glb:idle` | OK |
| `prod_missing_clip` | `walk` removed from production capybara | `production clips` | OK |
| `prod_single_channel_clip` | production `walk` reduced to one channel | `incomplete animation channels` | OK |
| `dev_unimportable_mesh` | every node's `mesh` reference removed | `failed to load mesh` | OK |

The harness is not attachment semantics. A clip that keeps any moving sampler passes the motion check, which is why the #73 contract must run as well.

### `tools/tests/asset_verification_workflow.py` (plain Python)

This is a static text invariant over `.github/workflows/*.yml` and `Makefile`. It requires:

- a workflow that runs `tools/build_assets.py -- --validate` under Blender;
- a path-scoped `pull_request` trigger with the required paths;
- both #73 runs on all ten candidates, plus the negative harness;
- `--python-exit-code 1` on every Blender `--python` invocation, in that workflow and in every Makefile target;
- Blender `5.0.1` and its archive SHA-256 checked with `sha256sum`;
- no Blender in `verify.yml`, on any unscoped trigger, or reachable from `make verify`.

The check was exercised against a scratch conforming workflow, which gave 0 failures. It was also exercised against these mutations, each of which was rejected:

- the `tools/tests/**` filter removed;
- `pull_request` left unscoped;
- `make verify` calling `assets-validate`;
- Blender added to `verify.yml`.

## Baseline observations (f621454, Blender 5.0.1 build a3db93c5b259)

| Item | Result |
| --- | --- |
| Generator `tools/build_assets.py` SHA-256 | `7bfc0ed67c0b6c49aec9c60e8a199bdae85c54d4cf8a04002223c9fbb48d083b` |
| `attachment_pose_blender.py` SHA-256 | `578f33c03b01da328103cf282d0155c280ff15d7e66aaf2c763e2dbc356021c0` |
| Existing validator | Passed: `validated 20 development packages and production GLBs`, rc 0 |
| #73 `--source`, all ten | 30 actual sources/exports, 0 failures, rc 0 |
| #73 `--generate --source`, all ten | 30, 0 failures, rc 0. The run left `tools/__pycache__` in the checkout, which was removed |
| Negative harness | **RED**, rc 1: 15 cases, 14 as expected, 1 unexpected (`catalog_budget_exceeded`), 0 tracked mutations (`/tmp/zoo75-negative-baseline.log`, `/tmp/zoo75-negative/asset-validation-negative-report.json`) |
| Static invariant | **RED**, rc 1: 4 failures. Three are Makefile `assets-development`/`assets-production`/`assets-validate` missing `--python-exit-code 1`. The fourth is that no workflow runs the validator |
| `make assets-validate` exit status with a missing buffer | **0** although the validator raised (`/tmp/zoo75-make-exit/make.log`) |

## Obligation mapping

| #75 obligation | Evidence |
| --- | --- |
| Wire validation into a path-scoped check | Static invariant (executable). The path-filter review item is below |
| No Blender or new environment overhead on unrelated PRs | Static invariant: `verify.yml`, unscoped triggers and the `make verify` closure stay Blender-free. Hosted proof is a review item |
| A broken model/buffer or over-budget output fails | Negative harness: 14 detection cases plus the positive control; `catalog_budget_exceeded` is RED. Fail-closed exit status is covered by the static invariant (`--python-exit-code 1`) and the observed `make assets-validate` rc 0 |
| Document actual coverage; SVG replay is not 3D verification | This file's coverage statement below. The implementer updates `assets/3d/README.md` and the `AGENTS.md` verification list. Review item |
| Semantic tests for #73 and #74 run in the check | #73: both all-ten attachment runs are required commands (static invariant). #74: **evidence gap**, see below |
| Reusable processing stays in asset-tooling; Zoo checks stay in Zoo | Review item. Every test here is a Zoo content check over Zoo's own catalog, and nothing here adds a processor, ledger or provenance format |

## Coverage statement (what a green check means)

The existing validator proves the following for each of the 20 catalog assets, in both stages:

- external buffers exist (byte length is checked only indirectly, through Blender's import);
- the GLB header, length and JSON chunk are valid;
- every POSITION float is finite;
- development, production and catalog triangle counts agree, and are within the in-code budget;
- the clip names are exact;
- each animated clip has at least two channels and at least one moving sampler;
- Blender can import a mesh.

The #73 contract adds attachment, pivot and rest-cycle semantics for the ten animated candidates, on both the saved and the freshly generated output.

The following are **not** covered:

- visual or art quality;
- geometry, material or animation equivalence between source and production (#74);
- browser or renderer integration;
- asset-tooling provenance receipts.

The existing `Verify` "Generate and replay-verify Zoo asset" step covers only the SVG park mark through asset-tooling. It is not 3D catalog verification.

## Independent review items (not locally executable)

1. **Path filter on GitHub.** Path filtering cannot be executed locally. On the implementation PR, the reviewer confirms that the new workflow ran because it touched a listed path. The reviewer also confirms that GitHub did **not** start the workflow on a PR, or a probe branch, that changes only an unrelated file (e.g. `apps/web/src/**` or `docs/**`), and that `Verify` on such a PR shows no Blender download step. Record both run URLs.
2. **Hosted RED evidence.** On the hosted runner, the reviewer confirms that the job fails when a step fails, for example from the negative harness's own non-zero exit at a RED head, or from a deliberately broken probe commit that is never merged. Green-only hosted evidence is insufficient.
3. **Blender identity in CI.** The job log shows the `sha256sum -c` success for the 5.0.1 archive and `Blender 5.0.1 (hash a3db93c5b259 …)`.
4. **Required check.** The new workflow becomes the required check for asset paths, the way `Foundation Fleet Audit` is for foundation paths (`AGENTS.md` verification list). No equivalent local command is claimed beyond the commands above.
5. **Boundary.** No generic glTF processor, processing ledger or provenance store is added to Zoo. If byte-length or schema checks are generic glTF semantics already available in asset-tooling or 3d-lab, consuming them at a pinned revision is preferred to re-implementing them. Zoo-specific budgets, clip names and catalog consistency stay in Zoo.
6. **Docs.** `assets/3d/README.md` and `AGENTS.md` state the actual coverage above, and no longer imply that `Verify` checks 3D assets.

## Evidence gaps

- **#74 semantics.** #74 (development-source edits must reach production) is `needs-decision`: the owner has not chosen the authoring authority. This contract defines no #74 oracle. Once that decision is recorded, its regression joins this check under the same path scope. The check must not claim #74 coverage before then.
- **`catalog_budget_exceeded`** is RED at baseline. The implementer must make the catalog's declared `triangleBudget` enforced: for example, reject any catalog entry whose `triangleBudget` differs from the authoritative budget, or whose observed triangles exceed it. The case must not be removed or its expectation weakened.
- The byte-length defect is currently caught only by Blender's importer. A future importer that tolerates short buffers would turn `dev_truncated_buffer` RED. That RED would be correct and must not be waived.
- Hosted path-filter behaviour, hosted RED, and the CI Blender download are review items 1–3; none are executed here.

No production, workflow or Makefile edits have been made in this context.
