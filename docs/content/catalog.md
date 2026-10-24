# Park content catalog

`crates/zoo-core/content/catalog.json` is the canonical content source. It carries no version number: the strict typed definition is its schema and the session `content_digest` its identity ([merge-safe format identities](../decisions/merge-safe-format-identities.md)).
Rust validates it for native and WASM parks. Vite serves and bundles this same
source at `content/catalog.json`; the browser fetches it and calls
`ZooGame.from_content_json` before creating a live park. A failed load displays
its validation error and creates no fallback park. `content_json` exports the
owned, validated definitions. Reset retains that park's catalog.

Species keys and facility keys are content identities, separate from runtime
entity IDs and localized labels. Species purchase, appeal, social and space rules
and stand construction/sale prices are consumed from the immutable per-park
bundle, as are each species' per-animal and each stand's hourly upkeep. The
`balance` section holds the park-wide numbers: admission price, path and habitat
construction, habitat care actions, feed batches, maintenance repairs, staff hire
costs and hourly wages, and per-habitat upkeep. Every money
field is validated with its field path (for example
`balance/staff/keeper/hourly_wage_cents`). Content of another shape (such as an
older catalog embedded in a save) fails with the offending field path; save
migrations rewrite embedded catalogs when the shape changes (schema 6 adds fences,
schema 8 drops the catalog's old version number, schema 9 adds habitat
environment prices, furnishings and species requirements), and saves keep the
catalog they were made with. Food/drink service behavior retains the existing core implementation.

`fences` (#95) lists the fence types a habitat can be enclosed with: key, label,
`material` (`wood` or `steel`, the supported looks), `height_cm` (1..=1000), and
per-segment build price and hourly upkeep. The first entry is the default for
every new habitat; the built-in catalog ships a 120 cm wooden rail and a 250 cm
steel mesh. Each species declares `minimum_fence_height_cm`; a habitat whose
fence is lower cannot adopt it, and the minimum must not exceed the tallest fence
type. A habitat owns a copy of its fence type, like its species definition.
Rebuilding a whole fence as another type charges that type's full per-segment
price, refunds nothing and is one undoable construction transaction; it cannot
drop below the resident species' minimum.

Habitat design (#97) is content too. `balance/habitat_environment` prices one
tile painted grass, sand, shallow water or deep water and one elevation step
raised or lowered. `habitat_objects` lists the one-tile shelters and enrichment
items a habitat can be furnished with: key, label, `role` (`shelter` or
`enrichment`), build price and how many animals one serves (1..=12); at least one
of each role must exist, and the first shelter is what *Add basic shelter* places.
Each species declares a `habitat` block: its preferred land `ground_cover` with
the share of usable land it should cover, the shallow and deep water tiles it
needs, and whether every animal needs a place in a shelter and a share of
enrichment. These are authored game simplifications, not veterinary claims.
Presentation references resolve against `assets/3d/catalog.json` and the existing
animal/stand presentation profiles. This extraction does not load new GLB assets
or change the accepted renderer. Fixed entrance and care-depot entries describe
the existing fixed facilities; they are not constructable concession services.
Unlock references currently support the existing available state only.

Malformed field types retain their typed Serde field paths via the
`serde_path_to_error` adapter; the existing typed definitions remain the sole
schema authority. Document bytes are bounded before parsing, with no parallel
raw catalog tree.

Supported loading budgets are 256 KiB per document, 128 entries in each group,
and 256 UTF-8 bytes per text field. Keys use lowercase ASCII letters, digits and
underscores. Unknown fields, duplicate IDs, unsupported versions/services,
dangling references and footprints other than the existing 1x1 fail closed.
These are implementation support bounds, not new gameplay balance decisions.

Cents fit the exact JavaScript integer domain (9007199254740991). Each economic
transaction checks cash and ledger additions before committing. Daily aggregate
income/expenses each fit half that domain so two-day profit differences remain
exact too. Failed construction/adoption transactions preserve cash and ledger;
failed sale/admission arithmetic does not award service or admission. Hourly
upkeep commits its four categories atomically. Stand lifetime revenue and sales
counts are checked before a sale. No counters wrap or saturate financial credits.
Species rule products use the existing park tile count and twelve-animal habitat
capacity to bound a conservative whole-park appeal sum in u32. Ordinary accepted
numeric rules and public command/tick outcomes remain unchanged.

This prerequisite does not implement scenario selection, persistence, recurring
care rules, unlock progression or the living-habitat product experience.
