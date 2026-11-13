---
name: numera-wiz
description: Use when a user wants to inspect media or a bounded folder, select or reorder original streams, clean or remux files, review and submit Numera Wiz plans or batches, observe or cancel accepted work, retrieve verified outputs, or explicitly administer Numera Wiz access. Uses the local Rust CLI or MCP bridge and the live command registry.
---

# Numera Wiz

Use the installed Numera Wiz Rust executable and its shared owner service.
FFmpeg/ffprobe perform admitted media operations. FFmWiz and MuxCls describe
required behavior; their original programs are not runtime dependencies.
This package adds instructions and client metadata, not another media engine.

## Discover the current contract

1. Connect to the configured `numera-wiz` MCP server, or identify the exact
   executable, data directory, components directory and named access profile.
2. Call `commands.list` with `{"params":{}}`. For each needed command, call
   `commands.schema` with `{"params":{"id":"COMMAND_ID"}}` and read its
   current `state_class`, parameter schema and qualification text. Do not infer
   support from tool names, a donor feature, this skill or an old example.
3. The configured `agent` profile permits discovery and its own replay-scope
   bootstrap only. Tool enumeration is not an authorization grant. For media
   work, use an owner-configured operating profile with explicit command,
   object and path rights. Read [access](references/access.md) before setup.
4. Preserve the selected data root/profile across discovery, planning,
   submission, observation and retry. Use the same identified binary/registry.
   Never remove a profile selector to obtain native-owner authority.

The Rust MCP adapter derives every advertised tool from the active registry.
Use newly registered operations only after inspecting their actual schema,
rights and qualification. [Operation routes](references/operations.md) maps
the currently implemented domains to workflows and records the CLI-only edges.

## Probe, plan and run

1. Confirm the user's input selection, desired stream behavior and absent
   destination. Call `media.probe` for inspection. Register the source through
   `assets.register` with a retained durable identity when a plan is needed.
   Accept exactly one native `path` or verified `upload_id`; a filename from
   an upload is not a native path. `assets.inspect` and `assets.output_suggest`
   help inspect the returned asset and propose a destination.
2. Build an explicit cleanup intent from the actual probe's original stream
   indices. Source index `0` is valid. Preserve independent choices for extra
   video, attached pictures, attachments, data, unknown streams, tags,
   chapters and dispositions. Start from
   [the keep-all intent](examples/cleanup-keep-all.json) only when it matches
   the user's intent. Read the live schema before changing selectors, partial
   orders or Keep/Set/Clear patches; do not invent a donor configuration parser.
3. Use `streams.clean.resolve` for a supplied stream inventory, or
   `streams.clean.report` for 1-64 registered assets. A report previews
   selection; it does not approve output compatibility or submit work.
4. Call `streams.clean` with the registered `asset_id`, authorized `output`,
   complete `intent`, and any explicit `file_times`/`execution_limits`.
   Inspect the returned plan using `plans.inspect`. Review input identity,
   selected/dropped original indices, patches, preservation claim, destination,
   exact argv, timeout policy, warnings and verification scope. Copy, packet
   remux and decoded render have different claims. A metadata-only change
   must not be described as a byte-identical copy.
5. Submit `jobs.submit` with the exact reviewed `plan_id` and `plan_hash`
   using its retained durable identity. Carry out an already authorized
   operation when the reviewed plan matches that scope. Resolve material
   ambiguity or newly required authority before execution; do not silently
   change the user's requested semantics to make a plan admissible.

There is no universal `run` tool. `jobs.submit` runs an admitted single plan;
`batches.submit` runs an admitted batch. Never replace either with a handwritten
FFmpeg invocation or a wrapper in another language.

## Watch, save and cancel

- Inspect `jobs.inspect` at a bounded interval until its returned state is
  terminal. A successful inspection can contain a failed producer job.
  Native CLI `jobs wait ID --timeout-seconds 1800` observes through the same
  query with a finite 1-86400 second waiting budget. Timeout/Ctrl+C stops
  waiting and leaves accepted work with its owner; the terminal wait result
  reflects the job outcome. It is not a new MCP watch/cancel operation.
  Report its actual stage/progress/error and verification/publication outcome.
  A finished FFmpeg progress stream, completed copy counter or exit code zero
  is not proof of published verified bytes. Do not invent duration ratios/ETA.
- Use `jobs.list`, `assets.list`, `batches.list` and `artifacts.list` only with
  their current page/cursor rules. Keep one snapshot's exact limit and cursor.
  Start a fresh snapshot after expiry rather than merging inconsistent pages.
- Obtain `artifacts.manifest` or read the advertised artifact manifest resource.
  Retain its exact digest and authorized artifact identity. MCP byte delivery
  uses `artifacts.read_chunk`: decimal-string `offset`, `length` 1-131072 and
  exact `expected_sha256`. A URI is a lookup location, not a permission.
- To save to the user's selected new local file, use the Rust CLI's
  `artifacts save --artifact-id ID --expected-sha256 SHA --out ABSOLUTE_NEW_FILE`
  under the same data root and named operating profile. Native client saving
  is not an MCP tool or an engine path grant. Without an authorized local
  filesystem/save facility, report that client delivery remains pending;
  provide the manifest and supported chunk route without inventing a save tool.
- Cancel an accepted job with durable `jobs.cancel` before its publication
  fence. Cancelling an MCP request or ending a client session does not cancel
  an accepted job. A cancellation race is resolved by subsequent inspection.
  Cancelling a client artifact save stops the copy, not its producer job.

## Durable identities and error handling

Read [MCP and CLI calls](references/calls.md) before mutating. Create an own
scope through `access.replay_scope.create`; retain the original command,
parameters, request ID, replay scope and idempotency key before first submission.
After an uncertain response, replay that exact identity to resolve its receipt.
Never allocate a new key simply because a timeout or disconnect occurred.

Read `structuredContent.ok`, `error.code`, receipt identity and returned object
facts. Protocol errors and domain errors have different envelopes. Treat
`PERMISSION_DENIED` as a missing or revoked grant; report the required right
without elevating the profile. For `IDEMPOTENCY_CONFLICT`, reconcile the original
request. For stale plans/input identity, re-probe/review rather than changing a
stored hash. For output collision, preserve existing bytes and select another
user-authorized destination. For `RESOURCE_LIMIT`, reduce the explicit set or
page/chunk size. For owner/store/version conflicts, preserve state and report
the typed failure; never delete a lock, state file or protected runtime record.

## Batches and owner administration

For folder intake, use durable `assets.register_folder` and query
`assets.folder_inspect`, following [folder intake](references/folders.md).
Review the immutable snapshot's completeness, every registration/probe failure,
exclusions and inventory-only companions before selecting explicit batch members.
Folder registration does not execute or publish a cleanup batch.

For a bounded explicit cleanup batch, inspect `streams.clean.batch_plan` and
the batch schemas, then plan all candidates with explicit failure/unresolved
policies. Inspect `batches.plan_inspect`, including every admitted, skipped or
rejected member, and submit the exact frozen ID/hash once. Observe
`batches.inspect` and child jobs; CLI `batches wait ID` observes without
cancelling accepted work on Ctrl+C. Use durable `batches.cancel` only when
the user's cancellation scope includes those members. This is not an implicit
recursive folder or companion-file scanner.

Access policy/token administration, internal credential issuance, protected
logging and store-generation maintenance require protected local-owner
authority. Do not call them merely to get media work unstuck. For an explicit
administration request, use [access setup](references/access.md) and the current
schemas. Keep credentials in protected new files and out of transcripts/config
commits. Preserve exact revision guards and retained migration/retry documents.
There is no MCP permission to self-select the owner principal.

## Report the evidence boundary

State the selected profile, plan/job/artifact identifiers, actual outcome,
verified preservation scope and material limits. Do not expose credentials,
protected paths or diagnostics omitted by the scoped view. Full FFmWiz F01-F15,
complete standalone/embedded MuxCls, editing/preview, standard offline packages
and every hardware route remain broader qualification obligations. A generated
schema, this package, a source inspection or successful client startup does not
prove those capabilities. Use the current registry and actual job evidence.
