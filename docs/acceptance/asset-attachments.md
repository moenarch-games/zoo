# Technical source attachment acceptance — #73

Authority: issue #73, existing content source `tools/build_assets.py`, asset-stage documentation, and `docs/architecture.md`. Baseline is default `65d3d68b5958a65fd2489c7824f9634add161c90`. This independently authored contract precedes the next source correction. Shared conventions resolved without diagnostics at sourceRevision `46d8793bb3034326561f876dcc67dbaa5aa1e432`; all 25 resolved files and local AGENTS/CLAUDE were read.

This is technical anatomy/attachment evidence for the existing procedural candidates, not expressive-art approval, licensing approval, canonical asset-tooling provenance, browser integration, or completion of #112/#140/#142. #74 remains the separate editable-source-to-production authority correction, and #75 remains verification integration. No product-local renderer, animation sampler, source normalizer or processing ledger is introduced here.

## Immutable outcomes; translatable source names

`tools/tests/attachment_pose_blender.py` loads actual saved Blender sources and actual development glTF/packed production GLB through Blender's own importer. It evaluates existing real idle/walk actions at frames 1, 7, 13, 19 and 25. These are samples of the currently authored one-second clips, not new gameplay cadence, FPS or motion budgets. The importer evaluates transforms; the test does not implement generic animation interpolation.

- Four human sources: each hand follows its arm, each shoe follows its leg, and eyes/nose/cap follow the head.
- Four quadrupeds: each hoof follows its corresponding leg; facial features, ears, cheeks and ossicones follow the animated head. Bird eyes/beaks/pupils follow the head; flamingo feet follow legs. Penguin wings pivot at their shoulder-side endpoint.
- Moving dependent parts inherit the corresponding moving subtree and preserve their rest-relative transform through intermediate poses. Motion must actually occur; freezing a part cannot pass. The elephant trunk may have independent secondary animation but must inherit the head. Giraffe/flamingo heads inherit their animated necks.
- A limb's proximal material point, derived from actual rest geometry, stays anchored relative to the root while the limb rotates; center rotation fails. No future implementation's joint position/name becomes the oracle. Current upright limb geometry identifies the proximal end as its upper cap center. Equivalent source representation/name changes require independent semantic translation, retaining anatomical attachment, actual movement and anchored pivot outcomes.
- Actual rest geometry is finite and both clips return to their rest transforms. Computed transform precision uses `2e-5`, appropriate to exported single-precision geometry; it is not a visual/style or timing acceptance threshold.
- Both export stages and saved source are evaluated. `--generate` additionally executes the unchanged actual source generator/export functions into an external output directory, then imports those outputs. Only destination constants are redirected; no geometry, hierarchy, animation, clip merge, exporter or preview implementation is replaced. Source action selection wraps real per-object actions in evaluation-only NLA strips; exported merged clips are loaded as authored. No successful outcome is injected.

SHA-256 observations identify each actual source/export and external glTF buffer. The original generator's SHA is recorded and checked after source generation. Blender renders actual evaluated imports at rest and intermediate poses when `--pictures` is requested; these are detached technical inspection views, never substituted for the living-habitat proof.

## Run

Use the already verified Blender 5.0.1 binary (archive SHA-256 `8019580ee1b7262e505f4196a00237ccf743c88d205b38d34201510676e60b09`; binary reports build `a3db93c5b259`). Blender Python is the necessary existing generator/importer runtime; this Python exception does not introduce a TypeScript-incompatible production toolchain. External operation bounds below are operational completion guards, not product performance limits.

```sh
BLENDER=/home/moenarch/.local/share/coding-agent/video-84-tooling/blender-5.0.1-linux-x64/blender
# All ten current animated candidates, both derivatives and saved source.
timeout -k 10s 300s "$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python tools/tests/attachment_pose_blender.py -- --root "$PWD" \
  --output /tmp/zoo73-attachments --source
# Actual source regeneration and real imported-pose pictures; no runtime-outcome fixtures.
timeout -k 10s 300s "$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python tools/tests/attachment_pose_blender.py -- --root "$PWD" \
  --output /tmp/zoo73-generated-attachments --assets guest capybara --generate --source --pictures
# Existing catalog structural/importability coverage remains necessary.
timeout -k 10s 120s "$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python tools/build_assets.py -- --validate
python3 -m py_compile tools/tests/attachment_pose_blender.py
```

Output must be a caller-owned disposable directory. Verification does not overwrite tracked sources/assets. The source-generation experiment writes only there. Do not waive errors or reuse stale pose reports as a pass. The source fix's merge gate must rerun the entire ten-candidate contract, actual generation/visual inspection, affected asset validation, applicable repository checks and required hosted gates. Wiring Blender into unrelated verification is outside #73 and must respect #75's path scope.

## Actual preproduction observations

At baseline, the existing validator passed all 20 development packages and production GLBs (`/tmp/zoo73-existing-validator.log`); it only checks structure/motion/importability and therefore misses this defect. Actual all-candidate execution inspected 30 inputs, with all 30 finite/rest-cycle observations positive, but **381 collected hierarchy/attachment/pivot failures**. This is one RED process with all inputs actually inspected, not 381 separately run tests. `/tmp/zoo73-protected-baseline.log`, report `/home/moenarch/.local/share/coding-agent/zoo73-protected-baseline/attachment-report.json` and timestamped `baseline-provenance.json` retain the evidence.

Generator SHA-256: `4c3db740f9f3ec204d988f240dacf8fc9357a45e3170340a3ca11a0291349cd9`. Independently executed actual source regeneration for guest/capybara on pinned Blender 5.0.1: six real source/export inputs, **84 collected failures**, finite rest and exact rest-cycle positives; actual rest/intermediate rendered pictures are retained under `/home/moenarch/.local/share/coding-agent/zoo73-source-final-baseline` (`/tmp/zoo73-source-final-baseline.log`). Saved tracked `.blend` files report being authored by a newer Blender build; this is disclosed, not hidden. Fresh source-generation evidence avoids inferring behavior from that compatibility warning or from packed files alone.

The complete corrected ten-candidate generation result, corrected visual evidence, current source-edit propagation (#74), canonical processing receipts, expressive-art approval and living browser scene are UNRUN/unavailable at this acceptance-only head. No production edits have begun in this context.
