# Validated catalog prerequisite acceptance (#145)

Independent acceptance only, draftPR144. Original species subset authored by
`/root/zoo_acceptance` at5fa815c againsta5938e5; extended by separate context
`/root/video_172_acceptance` against current defaultmain0748f06. No production
implementation patch supplied. Shared conventions sourceRevision
46d8793bb3034326561f876dcc67dbaa5aa1e432. Authority: #145, #90, existing public
commands/accepted numeric rules, `AGENTS.md`, `docs/architecture.md` and current
`docs/zoo-product-experience.md` (no Decision Atlas exists on this baseline).

This prerequisite owns a versioned Rust-validated species/facility catalog,
stable content IDs, assets/unlock references, valid supported footprints/costs,
bounded loading and immutable in-memory rules identity. Runtime recurring work,
care cadence/default priorities and habitat overrides remain #90/#104. Exact save
rules are #88. No new scheduler, save format, renderer, scripting runtime, proxy
asset or fake frontend/WASM simulation authority is introduced. Completing #145
must not close #90/#91/#96 or claim the living-habitat visual journey.

## Protected native acceptance

`crates/zoo-core/tests/content_contract.rs` uses the existing command boundary and
proposes content_json()/from_content_json() public adapters. These must export
validated definitions/create a park with the same Rust validation used by WASM;
not test-only methods. JSON/API spellings are independently authored routine
translations and may be independently translated without changing expectations.

Existing species purchase/appeal/social/space values remain unchanged. Food/drink
stand costs18000/14000cents, sale prices500/350cents and1x1 placement footprints
come from baseline source, not new balance decisions. Versioned definitions cover
existing fixed facilities/assets as well as constructable stands; references are
resolved against verified real Zoo/asset-tooling/3d-lab content, not arbitrary URLs
or permissive rendering fallback. Default unlock behavior preserves current
availability; missing references fail with useful field paths. No launch unlock
progression is invented by this technical extraction.

A canary species clones existing real species presentation/welfare and changes
stable ID/purchase price. A canary food facility clones real food presentation/
service behavior and changes stable ID/cost/price. Both must work through actual
adoption/place_concession commands, authoritative finance and idempotency. Merely
exporting a list or rendering a named proxy fails this contract.

Duplicate IDs, unsupported versions, malformed input, dangling asset/unlock
references, invalid footprint dimensions, negative costs, values that can overflow
supported financial arithmetic and16MiB labels fail closed before a park exists.
Implementation defines and documents finite byte/count/string/numeric limits;
16MiB is an oversized negative case, not a new universal content budget. Useful
field paths identify invalid data. Native tests do not introduce fake asset files.

`content_baseline.rs` and `fixtures/content-command-baseline.json` protect actual
0748f06 public-command outcomes for each of six species: build rectangle(3,8)-(7,10),
hire/schedule keeper, buy feed, adopt one animal, place food(1,6)/drink(3,6),tick137.
Expected cash, finance, welfare/habitat, facility wear/sales, guests, keepers and
operations were independently captured from unchanged baseline before implementation.
The capture mode was removed; ordinary tests cannot rewrite/pass their own oracle.
Loaded default catalogs must match those fixed outcomes. Cosmetic localization
edits remove only named presentation messages from comparisons, preserving numeric
state/IDs/guest decisions and all other simulation results. No test claims RNG
stream bytes are exposed; unchanged public trajectories establish decision parity.
Immutable owned per-park bundles must survive later source-definition edits.

## Actual WASM/browser integration

`apps/web/scripts/check-content-wasm.ts` consumes real generated Rust bindings and
WASM bytes; the same command/tick fixture must match native oracle through the
WASM public catalog adapter, and invalid asset references must be rejected by Rust.
This is adapter evidence, not a full browser journey by itself.

Reuse `capture-park-view-proof.mjs` with ZOO_CONTENT_ACCEPTANCE=valid/invalid.
The optional contract starts the real browser, reads the actual production bundled
catalog over HTTP and intercepts only that catalog request with a protocol-faithful
JSON200response containing independently chosen canary records. All asset references
come from real accepted entries. The actual production loader/WASM validates the
response and the real UI selects/builds the new facility and adopts the new species.
No window game-state injection, fake WASM, simulated command responses, generated
art or content-editor feature is used. Unaffected default dogfood remains unchanged.

Proposed routine bundled-loader path is content/catalog.json relative to preview;
ZOO_CONTENT_CATALOG_PATH can translate the endpoint independently. The file/API
spelling is flexible; actual production validated bootstrap and catalog-driven UI
are essential. Fixture and normal app must share that loader. Dynamic tools must
consume Rust costs/rules rather than a copied TypeScript food/drink/species switch.
The canary browser construction price is12300cents (whole-dollar HUD supports an
exact123dollar delta while paused); native canary checks12345cents through realcash.
Repeated placement must not charge twice. The standard canvas/inspector/keeper/
adoption journey remains real. Invalid canary asset_ref must show a useful field
error and never silently start a fallback park. Invalid existing live parks must
remain untouched when applicable; no save-loading claim is made.

Browser CDP fixtures are DATA authority only; they do not establish recognizable
new art or full #91 starter-park acceptance. Runtime entity IDs remain separate from
content keys; presentation/localized names never become simulation discriminants.

## Commands, initial evidence and remaining audit

- cargo test -p zoo-core --test content_baseline --locked
- cargo test -p zoo-core --test content_contract --locked
- make wasm; bun apps/web/scripts/check-content-wasm.ts
- production preview plus ZOO_CONTENT_ACCEPTANCE=valid bun apps/web/scripts/capture-park-view-proof.mjs
- same with ZOO_CONTENT_ACCEPTANCE=invalid
- make verify and existing browser visual/asset consumer gates before merge.

Initial baseline: protected six-species/facility oracle green; new catalog tests
compile-red because content APIs do not exist. Generated WASM/catalog bootstrap
and browser canary proof remain unavailable/unrun; missing runtime seam is not a
pass. Script syntax, Rust formatting and diff checks validate acceptance authoring,
not gameplay completion. Current impacted relation mapping is incomplete, so run
full applicable documented validation once implementation exists.

Manual architecture review must confirm bounded allocation before parsing huge
catalogs, no overflow across supported command/tick trajectories, actual asset/
unlock reference closure, immutable bundle ownership, no copied frontend rules,
and baseline-compatible simulation decisions. Browser invalid-catalog failure must
remain observable, with no permissive default catalog fallback. No changes to
provenance/identity expectations may be made to get a green test without independent
specification verification.

## Supplemental economic boundary audit after implementation

Independent supplemental tests authored after code-complete `7988b30`, not a
pre-implementation RED contract: `economic_boundary_acceptance_tests.rs`, included
only inside the existing private test module. Actual private GameState helpers
exercise exact cash-edge/aggregate-edge success followed by atomic rejection,
negative expense rejection, all four upkeep charges failing after initial categories,
lifetime revenue and both u32 sale counters, cash/daily-ledger sale failure, and
exhausted guest identifiers. Failed transactions compare the entire debug state,
including hidden authoritative fields, not only HUD cash. Sale fixtures first create
an actual attraction/guest/stand through existing commands and spawn logic; a reset
positive control actually buys the drink, preventing an inactive fixture from passing.
These tests do not prescribe a new content budget, balance default or persistence API.

Source audit: catalog cents are nonnegative and at most exact JavaScript MAX_SAFE_INTEGER
9007199254740991. Spend/earn stage both checked cash and ledger before committing;
ledger nonnegative income/expense totals each remain at most floor(MAX/2). Hence
profit lies in [-floor(MAX/2),floor(MAX/2)] and a two-day difference has magnitude
at most 2*floor(MAX/2)=MAX-1, still exactly representable in JS. The independent
boundary test pins this arithmetic rather than assuming the constant is safe.
Concession sales preflight lifetime revenue and both counters before income and
before guest restoration, wear, litter or maintenance side effects. Guest exhaustion
precedes admission credit. Upkeep stages all four categories before cash/ledger
commit. Default command/tick numerical parity remains separately protected by the
pre-implementation six-species oracle; supplemental boundaries do not replace it.

These are runtime arithmetic safety checks on controlled internal boundary inputs,
not claims of save compatibility, exactness for every nonfinancial elapsed counter,
new UI affordances, or a completed #91 visual journey. Normal supported inputs and
public fixture trajectories remain mandatory native/WASM/browser gates.

## Wrong leaf types retain useful diagnostics

Supplemental negative cases independently authored against frozen `54e0e9c`
(code-complete production `7988b30` plus mechanical test corrections), before the
wrong-type diagnostic repair. Existing useful-path acceptance already applies to
these inputs; this adds executable coverage without changing the approved catalog.
Native and actual generated WASM both mutate only one field in valid exported
catalog data. Numeric/bool values where strings are required, and string/bool values
where rule numbers are required, must reject and include the offending leaf token.
Covered fields: schema_version/ruleset_id, species and facility key/label/asset_ref/
unlock_ref, species appeal/minimum_social_group/space_per_animal, facility service
and footprint width/height. Generic serde rejection without a field is insufficient.
Exact error prose/wire format is flexible; a useful offending field is mandatory.

Both frozen native and real WASM runs are genuinely RED at the first schema_version
case: `catalog: invalid type: string "bad", expected u32` omits schema_version.
Subsequent assertions remain unexecuted in these fail-fast runs. Root/producer's
separate observed asset_ref case reports the same class of gap; it is not claimed
independently executed here. Full case traversal must pass after the repair. The
original fixed simulation oracle, native canary, real WASM parity and browser
bootstrap/invalid-catalog requirements remain unchanged. No production edits accompany
these tests; ordinary malformed JSON may still report syntax without a leaf.
