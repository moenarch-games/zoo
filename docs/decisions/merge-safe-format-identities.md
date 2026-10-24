# Merge-safe format identities

Owner-approved 2026-10-24.

## Problem

Parallel PRs each bumped a hand-written compatibility number to the same next value (in mmorpg two PRs both claimed schema 26 / revision 26). Git merges identical edits silently, so both PRs went green on their own and the second one broke `main` after it merged. There was no merge queue to re-test the combination at the time (the repositories have since moved to the moenarch-games organisation).

## Decision

No compatibility guard is a number someone edits by hand. Each identity is derived from the thing it guards:

- **Content catalog**: no version field. Serde's strict typed `Catalog` (`deny_unknown_fields`) is the schema, and the session recorder's `content_digest` (FNV-1a over the validated catalog) is its identity. Content of another shape fails with the offending field path.
- **Scenarios**: no version field, for the same reason; `Scenario::digest` identifies a scenario.
- **Park saves**: saves keep their ordered migration chain, because old parks must keep loading. This is a decision taken on the recommendation (2026-10-24): #88 promised that old parks keep loading, so zoo keeps migrations despite the general no-compat preference. `save.rs::MIGRATIONS` is an append-only list and `SCHEMA_VERSION` is its length plus one. A PR that changes the saved shape appends one migration and one immutable `tests/fixtures/save-v<n>.json`. When two PRs both do that, they append after the same last entry and add the same fixture path, so git reports a conflict and the second PR has to be rebased and put in order. It can no longer merge silently.

mmorpg follows the same rule. Its content revision is the computed content fingerprint, and its wire protocol carries a random identity token that gets replaced, never incremented, whenever the layout changes.

## Consequences

- A PR never writes a schema, revision or version number. If two changes are incompatible, git conflicts on the same line, so the clash is visible before merge.
- The save schema stays an ordered integer inside the document, but nobody types it. Migration functions do not stamp versions; `migrate` stamps `SCHEMA_VERSION` once at the end.
- Schema 8 is the migration that removed the embedded catalog's old `schema_version`. It was appended after #202's schema 7 on rebase, which is exactly the path this rule prescribes.

## Still numeric, on purpose

- `apps/web/src/park-storage.ts::DB_VERSION`: the IndexedDB API requires an increasing integer upgrade version.
- Asset manifest `schemaVersion` (`scenery-manifest.ts`, `effects-kit.ts`, `stage-asset-releases.ts`): an external contract owned by asset-tooling.
- `profiles/runtime-profiler/simulation.json` `schema_version`: an external profiler contract (and already a string).
