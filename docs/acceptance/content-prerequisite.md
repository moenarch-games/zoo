# Content and rules identity prerequisite acceptance

Independent pre-implementation subset of #90 needed by #91, baseline
`a5938e51ad17da792584d6e5b3a619cdf2e5ec8c`.
Shared conventions sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
Authority: issue #90 and existing six-species public snapshot/commands. This is
not a completion contract for the entire content roadmap or for #91.

`cargo test -p zoo-core --test content_contract --locked` exercises the native
public boundary used by the WASM consumer. Baseline is **compile-red** because
`ZooGame::content_json` and `ZooGame::from_content_json` do not exist; this is
missing capability evidence, not a demonstrated runtime assertion failure.

The proposed minimal public adapter exports a Rust-validated immutable content
bundle from `content_json() -> String` and creates a new park using
`from_content_json(&str) -> Result<ZooGame, String>`. It must delegate to the
same Rust validation in WASM; this is a routine seam proposal, not an owner-approved
wire schema. The test translation proposes schema version 1, a nonempty
`ruleset_id`, and `species` records using existing public catalog field names.
If #90 implementation requires a different representation, this acceptance
context must independently reverify equivalent translation before tests change.
Do not add test-only methods or a hard-coded acceptance species.

The tests cover preserved six-species costs/appeal/social/space rules,
versioned per-park rules identity, owned copies independent from source edits,
definition-only species adoption through real command/finance authority,
unsupported versions/malformed input/duplicates/negative costs/invalid welfare
validation, useful errors, and deterministic command/tick replay.

This subset unblocks a scenario loader's ability to bind valid species references
and a supported immutable rules identity. It does not prove facility definitions,
asset/unlock references, overflow handling, species care-schedule defaults and
habitat overrides, complete six-species gameplay parity, WASM/browser catalog
placement, or save compatibility. Those remain #90 work; do not close #90 from
these tests alone. Scenario creation should not bypass unresolved #90 semantics.

#88 remains authoritative for the save envelope and exact resumed rules. No
scenario-specific persistence format is introduced. #91 now has a native blockedBy
link to #90, reflecting its existing issue body. Save/resume cannot be claimed
before #88 integration.

When #91 resumes, distinguish an intentionally partly prepared park (unfinished
build opportunity, not necessarily broken operations) from a recovery park with
failed operations. Empty, partly prepared and living starts share the same format;
recovery is an optional additional fixture rather than a substitute. First objective
framing must use zoo-scene/shared camera APIs with immediate normal interaction.
