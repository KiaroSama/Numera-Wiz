# Research provenance and source policy

The owner selected a Pure Rust repository on 2026-11-11 and explicitly excluded
third-party program source snapshots from it, including compressed copies. The
old research pack and mandate contain historical preservation instructions;
this policy and the current owner's instructions supersede that storage choice.

`docs/research-pack/research-pack-manifest.json` schema 2 separates retained
`files` from `external_files`. Retained documents/manuals are verified byte for
byte against their original SHA-256 values by Rust automation. External rows
retain original names, sizes and hashes as provenance; their bytes are absent
from the repository and are not asserted to have been fetched or verified by CI.
Original reference bytes remain in the owner-supplied companion outside Git.

Use `docs/research-pack/research_v7/sources.lock.json` and the source manifests'
pinned upstream URLs to retrieve specific source evidence. Historical relative
links to excluded source files identify external records and no longer resolve
to repository files. Do not interpret those source inventories, archived research
claims or a passing hash check as application/runtime qualification.

The [current upstream review](upstream-review-2026-11-11.md) records 15 source
heads/releases, inspected patches, selected updates and explicit comparison gaps.
The Rust application currently adopts rmcp 3.9.0 and current immutable CI actions;
breaking editor/store/component upgrades retain separate compatibility gates.

Spec Kit and `specs/001-numera-wiz` are retained only in the local ignored
workspace. Public documentation links the product mandate and development
status; installing a plugin does not require access to private planning files.
