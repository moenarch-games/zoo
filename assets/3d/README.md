# Zoo 3D asset stages

`development/` contains editable glTF packages: a `.gltf`, its external `.bin`, and the Blender `.blend` source. `production/` contains the matching packed `.glb` runtime derivatives. Production is never edited directly.

The catalog covers only entities defined by `zoo-core` and the current UI: entrance, animal-care depot, food/drink concessions, fence, terrain markers, feed/litter props, six catalog species, a guest, and keeper/janitor/mechanic characters. Characters and animals export multi-part `idle` and in-place `walk` clips using named transform-node hierarchies: limbs, wings and necks pivot at their anatomical joint, and dependent details (hands, shoes, hooves, feet, facial features) are parented beneath the moving part they belong to. `tools/tests/attachment_pose_blender.py` checks those attachments at intermediate idle/walk poses. No textures are needed; a compact embedded PBR palette preserves the clean, readable low-poly style.

Triangle budgets are declared per asset in `catalog.json` and enforced for both stages (the validator checks both the catalog declaration and the generator's own budget table). Current budgets range from 400 triangles for repeated terrain tiles to 3,500 for the most distinctive animals. Detail is spent on silhouette, facial features, role props, rounded edges, and animation joints rather than invisible subdivision.

Build either stage independently:

```sh
blender --background --factory-startup --python-exit-code 1 --python tools/build_assets.py -- --stage development
blender --background --factory-startup --python-exit-code 1 --python tools/build_assets.py -- --stage production
blender --background --factory-startup --python-exit-code 1 --python tools/build_assets.py -- --stage all
blender --background --factory-startup --python-exit-code 1 --python tools/build_assets.py -- --validate
```

The generator records observed triangle and material counts in `catalog.json`. `--validate` checks every development buffer reference, packed GLB header, actual triangle count, finite position value, budget, animation name, and minimum multi-part channel count, then imports both formats through Blender. The model source generator is deterministic and has no network or model dependency.

Without `--python-exit-code 1`, Blender exits 0 even when the script raises, so a failed validation looks green.

In CI, the path-scoped `3D asset catalog` workflow (`.github/workflows/assets.yml`) runs this validation on SHA-256-pinned Blender 5.0.1 whenever the generator, its Blender tests, this directory or the Makefile change. Each run covers the following, and `docs/acceptance/asset-verification.md` lists exact coverage and gaps:

- `--validate`;
- the #73 attachment contract on the tracked outputs and on freshly generated candidates;
- the #75 harness `tools/tests/asset_validation_negative.py`, which proves the validator rejects broken buffers, corrupt GLBs, non-finite positions, triangle drift, over-budget outputs, motionless or missing clips and unimportable meshes.

The general `Verify` workflow does not run Blender, and its asset-tooling SVG replay covers only the park texture, not these 3D assets. Whether edits to development sources reach production (#74) is not yet verified.
