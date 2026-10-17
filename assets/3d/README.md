# Zoo 3D asset stages

`development/` contains editable glTF packages: a `.gltf`, its external `.bin`, and the Blender `.blend` source. `production/` contains the matching packed `.glb` runtime derivatives. Production is never edited directly.

The catalog covers only entities defined by `zoo-core` and the current UI: entrance, animal-care depot, food/drink concessions, fence, terrain markers, feed/litter props, six catalog species, a guest, and keeper/janitor/mechanic characters. Characters and animals export multi-part `idle` and in-place `walk` clips using named transform-node hierarchies: limbs, wings and necks pivot at their anatomical joint, and dependent details (hands, shoes, hooves, feet, facial features) are parented beneath the moving part they belong to. `tools/tests/attachment_pose_blender.py` checks those attachments at intermediate idle/walk poses. No textures are needed; a compact embedded PBR palette preserves the clean, readable low-poly style.

Triangle budgets are declared per asset in `catalog.json` and enforced for both stages. Current budgets range from 400 triangles for repeated terrain tiles to 3,500 for the most distinctive animals. Detail is spent on silhouette, facial features, role props, rounded edges, and animation joints rather than invisible subdivision.

Build either stage independently:

```sh
blender --background --python tools/build_assets.py -- --stage development
blender --background --python tools/build_assets.py -- --stage production
blender --background --python tools/build_assets.py -- --stage all
blender --background --python tools/build_assets.py -- --validate
```

The generator records observed triangle and material counts in `catalog.json`. `--validate` checks every development buffer reference, packed GLB header, actual triangle count, finite position value, budget, animation name, and minimum multi-part channel count, then imports both formats through Blender. The model source generator is deterministic and has no network or model dependency.
