# Registry routes and limits

This is a route map, not a frozen tool schema. Always discover the identified
binary's active registry and read `commands.schema` before use. Commands can be
advertised while the current principal cannot call them. Qualification applies
to each operation/media/transport/package boundary separately.

| Canonical operation | Workflow / authority |
| --- | --- |
| `commands.list`, `commands.schema` | Live implementation/schema discovery. |
| `access.replay_scope.create` | Bootstrap own durable retry identity. |
| `media.probe` | Inspect authorized native media with a finite probe budget. |
| `assets.register` | Durably register one native path or verified upload. |
| `assets.register_folder`, `assets.folder_inspect` | Durable bounded native folder snapshot and immutable creator-scoped inspection, including per-file failures/companion inventory. |
| `assets.list`, `assets.inspect`, `assets.output_suggest` | Authorized inventory, details and proposed absent output. |
| `streams.clean.resolve` | Pure explicit original-stream inventory resolution. |
| `streams.clean.report` | Bounded registered-asset selection report. |
| `streams.clean`, `plans.inspect` | Prepare/review one immutable cleanup plan. |
| `jobs.submit`, `jobs.inspect`, `jobs.list`, `jobs.cancel` | Run, observe/page and control accepted jobs. Submission/cancellation are durable. |
| `streams.clean.batch_plan`, `batches.plan_inspect` | Review 1-64 explicit batch candidates and frozen member outcomes. |
| `batches.submit`, `batches.inspect`, `batches.list`, `batches.cancel` | Run, inspect/page and cancel a reviewed batch. Submission/cancellation are durable. |
| `artifacts.list`, `artifacts.manifest`, `artifacts.read_chunk` | Authorized verified output discovery and bounded bytes. |
| `access.profiles.create`, `access.profiles.inspect`, `access.profiles.update`, `access.profiles.list` | Protected owner setup; update uses exact revision and full replacement grants. |
| `access.profiles.put` | Legacy protected-owner profile mutation; prefer create/update. |
| `access.tokens.issue`, `access.tokens.revoke` | Protected owner file-delivered audience credentials and selected revocation. |
| `access.credentials.issue` | Internal scoped bridge credential bootstrap; not an agent export mechanism. |
| `logs.status` | Protected-owner logging health only. |
| `maintenance.plan`, `maintenance.migrate`, `maintenance.inspect` | Explicit protected-owner stopped-root generation workflow; preserve raw plan/retry identity. |

The Rust CLI also implements `doctor`, desktop/browser startup, bridge startup,
protected file-delivery helpers and `artifacts save`. They are composition/client
operations rather than additional MCP tools. Accepted-job observation is repeated
`jobs.inspect`; there is no `jobs.watch` MCP operation. `batches wait` is a native
CLI observer over the batch inspection contract. `jobs wait` adds a bounded
native-client observation loop over `jobs.inspect`; it is not a server mutation.

Cleanup selectors preserve original-index semantics, separate audio/subtitle
partial ordering, original language/title predicate matching, ordered per-field
tag patches, independent preservation choices and explicit no-match handling.
Language matching is normalized exact matching (`en` differs from `eng`); title
matching is substring OR, not regex. Unlisted retained streams append in source
order; missing/dropped indices do not reappear. A no-match skip produces no
artifact. Read the current schema for the exact tagged-variant fields.

Keep-everything can plan a verified Rust byte copy. Current admitted remux uses
FFmpeg and bounded packet/structure/tag assertions with exact rational clocks;
it does not imply universal container/codec or decoded-content identity.
File-time policies are `new`, `preserve_best_effort`, `require_reviewed`; they do
not clone ACLs or change packet clocks. Exact deadlines and file-time snapshots
that affect a plan must be reviewed with its immutable hash.

Batch planning consumes explicit registered candidates with explicit failure and
unresolved-index policies. Per-member execution/verification outcomes remain
visible; partial failure must not be summarized as full success. The bounded
folder intake records at most 32 manifest entries and inventories/excludes
companions; it does not associate/publish companion copies. Full folder and
companion FFmWiz/MuxCls behavior still requires broader implementation/evidence.
Do not synthesize an unregistered conversion/editor command by passing raw
FFmpeg argv through a cleanup schema.

The standard offline payload, complete F01-F15, full MuxCls, editing/preview,
cross-client/package parity and owner hardware qualification remain broader
obligations. They are not supplied by adding plugin metadata. A missing active
command or `CAPABILITY_UNAVAILABLE` is a concrete implementation boundary.
