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
| `media.analyze.volume`, `media.analyze.audio_tracks` | Measure a registered audio stream or compare decoded signals across selected original audio tracks. |
| `capabilities.inspect`, `capabilities.refresh`, `capabilities.clear` | Read or refresh FFmpeg/ffprobe observations and clear a matching component cache entry. |
| `media.preview_frame` | Decode one frame of an authorized registered video stream when the active registry provides this command. |
| `timeline.preview_frame` | Read one exact visible timeline frame when available, with project-context and source grants. |
| `timeline.render_mixed` | Review a committed sequence range for a mixed FFV1/PCM export on an enabled MediaV2 generation. |
| `assets.register` | Durably register one native path or verified upload. |
| `assets.register_folder`, `assets.folder_inspect` | Durable bounded native folder snapshot and immutable creator-scoped inspection, including per-file failures/companion inventory. |
| `assets.list`, `assets.inspect`, `assets.output_suggest` | Authorized inventory, details and proposed absent output. |
| `streams.clean.resolve` | Pure explicit original-stream inventory resolution. |
| `streams.clean.report` | Bounded registered-asset selection report. |
| `streams.clean`, `plans.inspect` | Prepare/review one immutable cleanup plan. |
| `streams.extract` | Review packet-copy extraction of one registered stream to a fresh Matroska path. |
| `streams.extract.batch_plan` | Review separate Matroska outputs with a per-file track type and ordinal. Submit the approved ID and hash via `batches.submit`. |
| `config.import.ffmwiz` | Parse FFmWiz settings and read field diagnostics from the versioned import report. |
| `chapters.inspect`, `chapters.export`, `chapters.import`, `chapters.remove` | Inspect exact chapter ticks, export FFmetadata or review chapter replacement or removal. |
| `chapters.wave_cues` | Read original RIFF or RF64 cue offsets, lengths and labels from an authorized asset. |
| `disposition.patch` | Review a stream-copy plan with explicit audio or subtitle flag changes. |
| `metadata.patch` | Review explicit container or original-stream tag changes for a fresh Matroska file. |
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

## Common registered-media queries

- To enumerate FFmpeg codecs and filters while leaving hardware idle, use
  `capabilities.refresh` with `{"params":{"components":true,"devices":false}}`.
  Read `advertised` and `cpu_round_trip` separately. `hardware: "unknown"`
  means the request made no device observation. A device request needs
  explicit native-owner authority and a user request to check the hardware.
- For a chapter-only sidecar, use `chapters.export` with
  `{"params":{"asset_id":"ASSET_ID","include_global_metadata":false}}`.
  Return the `ffmetadata` text and its `sha256`. `chapters.inspect` supplies
  the original ticks and rational time bases for review.
- To import a chapter document when `chapters.import` is active, pass the text
  in `ffmetadata`, the registered `asset_id` and a fresh `.mkv` `output`.
  Set `include_global_metadata` to `true` only to apply the document's listed
  container tags. Review the exact chapter ranges before submitting the plan.
- To make original audio stream 2 the sole default of its type, use
  `disposition.patch` with
  `{"params":{"asset_id":"ASSET_ID","output":"ABSOLUTE_NEW_FILE","patch":{"stream":2,"exclusive_default":true}}}`.
  Confirm the original stream kind in the source probe and check the returned
  flags before submitting the reviewed plan. Its peers lose only `default`.
- To set a tag on original stream 2 when `metadata.patch` is active, use
  `{"params":{"asset_id":"ASSET_ID","output":"ABSOLUTE_NEW_FILE.mkv","patch":{"target":{"scope":"stream","index":2},"set":{"title":"Commentary"}}}}`.
  For a container tag, set `target` to `{"scope":"global"}`. Put tag names to
  delete in `remove`. Review the returned plan before submitting its ID and hash.
- To remove the second audio track from several files, use the shared intent
  in `streams.clean.batch_plan` with `audio_selection` set to
  `{"selector":{"mode":"exclude_type_ordinals","values":[1]},"no_match":"reject"}`.
  Resolve each requested position before reordering streams. Review each
  candidate's outcome and retain the approved output paths, plan ID and hash.
- To read one decoded source frame, query `media.preview_frame` with `asset_id`,
  `stream_index` and zero-based `frame`. The result carries a 160 × 90 RGBA
  frame in `rgba_base64` together with its source content digest.
- To view one timeline frame, discover `timeline.preview_frame` and supply the
  project in `context.project`, plus `sequence_id`, current `revision`, and
  `at` as an exact sequence-frame time. The result identifies the visible
  registered or solid-color occurrence, or a gap.
- For a mixed export, supply the project, committed sequence revision and exact
  reviewed range to `timeline.render_mixed`. Check the returned source bindings
  and output profile. Submit the exact plan JSON as `jobs.submit`'s
  `media_plan_json` and retain it with the replay identity. Use
  `artifacts.manifest` for the accepted job's verified output.

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
