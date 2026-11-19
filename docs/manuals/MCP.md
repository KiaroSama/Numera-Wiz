# MCP manual - development implementation

The adapters use official rmcp 3.10.0 and the implemented 2025-11-25 initialize and
2026-07-28 discover lifecycle profiles. They forward typed operations to the same
protected owner as the native/browser UI and CLI. Hosted integration runs actual
media jobs through stdio and both direct-HTTP lifecycle profiles. Named external
client configurations and complete five-surface/package qualification remain open.

## Stdio

Launch the identified executable with `bridge`, optionally supplying a stored
`--access-profile operator`. The launch profile determines the stable effective
principal. Incoming tool params cannot select owner authority or another profile.
A conventional client configuration, when that client supports `mcpServers`, is:

```json
{"mcpServers":{"numera-wiz":{"command":"G:\\Program Files\\Numera Wiz\\numera-wiz.exe","args":["--data-dir","G:\\Numera Data","bridge","--access-profile","operator"]}}}
```

The default `agent` is discovery-only, apart from own replay-scope bootstrap. The
owner creates explicit operating profiles with `access profiles create` before
using them. A profile-based stdio launcher obtains a scoped private credential; it never
forwards its native owner credential to the agent. Rotation/restart preserves the
stored profile's principal. An idle bridge alone does not retain the worker.
Use `access profiles inspect --name operator --grants-out <new-file> --json`
and revision-checked `access profiles update --grants-file <file>` to replace its
complete policy while preserving existing path/operation grants. Plan read/submit
and job read/control are separate rights. Lists use authorization-bound 30-second
summary snapshots; they do not return every full record or infer access from IDs.

For deliberately authorized local owner automation, launch
`numera-wiz --data-dir <absolute-root> bridge --native-owner`. This explicit mode
uses the same protected native-owner connection as the owner CLI, including
implemented access administration and generation-maintenance commands. It uses
the same canonical requests, reviews, receipts and resource limits. It is stdio
only and rejects `--access-profile`, `--token-file` and HTTP combinations before
initializing a data root. Tool parameters cannot enable it or change authority.
The shipped agent configurations continue to select their scoped profile.

An owner can set this argument array in a private local client configuration:

```json
["--data-dir","<absolute-root>","bridge","--native-owner"]
```

Select that mode in the client configuration deliberately; a permission refusal
does not authorize an agent to replace its scoped connection with an owner one.
Actual named-client loading remains separate from hosted official-SDK evidence.

`access.tokens.issue` and `access.credentials.issue` additionally require an
absolute `credential_file` beside `params` in the MCP tool arguments:

```json
{"params":{"for_access_profile":"operator","port":1578},"credential_file":"<absolute-private-output-file>"}
```

Only explicit native-owner stdio performs this delivery. Scoped and HTTP bridges
refuse before filesystem or issuance effects. The shared CLI delivery path writes
a protected file without replacement; the result contains its path, token ID and
non-secret metadata. Credential values never appear in MCP results or text.
Publication failure attempts revocation of the issued credential. After a lost
response or cancellation, inspect the original output before retrying; an existing
file is a collision and is never overwritten. Canonical issuance parameters and
the credential's original profile/audience rules remain unchanged.
Cancellation already observed before the issuance request prevents delivery.
After that request starts, the adapter keeps processing its result for protected
publication or compensation. Forced disconnects still leave outcome uncertainty.

## Typed calls and retries

`tools/list` is generated from the active shared registry. Follow each
`nextCursor` with a `tools/list` request containing that `cursor` until the
response omits `nextCursor`. The official Rust SDK's `list_all_tools()` does
this automatically. Each JSON-RPC page fits within the 256 KiB response budget.
The `commands.list` tool returns the complete catalog of active command IDs,
metadata and the registry fingerprint in one bounded response. Call
`commands.schema` with an ID for its generated parameter and result schemas.
Tool input uses
`{"params":{...}}` and optional `context`. For example, discovery is
`{"params":{}}`. A durable call has this shape:

```json
{"params":{"path":"G:\\Media\\source.mkv"},"context":{"request_id":"import-1","replay_scope":"<issued scope>","idempotency_key":"import-source-1"}}
```

Obtain the scope first with `access.replay_scope.create` under the same profile.
Keep the original command, params, scope and key after a timeout/cancel/disconnect.
A possibly accepted mutation is resolved through its durable receipt; changing the
key is new work. SDK cancellation and accepted-job cancellation have different
lifetimes. Use `jobs.cancel` to control an accepted job before its publication fence.
Domain failures are structured tool errors. Invalid protocol envelopes use SDK
protocol errors. No custom MCP wire parser or lifecycle implementation is used.

`media.analyze.volume` accepts `asset_id` and the original `stream_index` of a
registered audio stream. It measures the selected decoded signal through FFmpeg
and reports peak and RMS to six decimal places as integer millionths of a dBFS.
A silent signal is represented as `{"kind":"silence"}`. The response includes
the registered source digest and a decimal-string sample count per channel.

`media.analyze.loudness` accepts the same registered asset and original
stream index. It measures EBU R128 values through FFmpeg. The report expresses
integrated loudness and gate in hundredths of a LUFS, true peak in hundredths
of a dBTP and range in hundredths of an LU. The integrated value is null below
the measurement gate. Digital silence returns a null true peak.

`media.analyze.reference` takes `reference` and `candidate` windows with
`asset_id`, `stream_index` and zero-based `start_frame`, plus `frame_count`
from one to eight. The registered video geometry and the decoded presentation
grids must agree. The engine preserves the decoder clock and rejects format
changes during frame selection. Each score has the exact relative time and both original
frame indices. PSNR is in hundredths of a dB or `{"kind":"identical"}`.
SSIM is reported in millionths. The scores describe decoded 8-bit RGB.

`media.analyze.stream` takes `asset_id`, `stream_index`, `kind` (`packets` or
`frames`), zero-based `first` and a `limit` from one to 64. The owner scans the
selected stream through ffprobe, returning the number of rows emitted as a
decimal string plus the requested rows. Times are exact rationals on the registered
stream clock. Packet hashes are SHA-256 of the original payloads. A scan that
reaches a processing limit returns a typed error.

`media.analyze.audio_tracks` accepts `asset_id` and two to sixteen distinct
original audio indices in `stream_indices`. It compares SHA-256 measurements
over packed `pcm_f64le` samples, sample rate and channel layout. The response
returns original indices grouped by exact decoded matches. Tracks whose
multichannel layout is unavailable retain their signatures for inspection.

`streams.extract` reviews one registered video, audio or subtitle stream for
packet copy to a new Matroska path. Pass `asset_id`, the original `stream_index`,
and the granted `output` path. Inspect the plan and submit its exact ID and
hash with `jobs.submit`. The output preserves metadata and dispositions for
the chosen stream and omits the other streams and chapters.

`streams.extract.batch_plan` takes `items` with a `target` containing
`asset_id` and `output`, plus a `selector` with `kind` and one-based
`ordinal`. Resolve each target against its own registered probe, including
assets from a folder scan. The review preserves the requested selector and
resolved original index or rejection. Set `failure_policy` to `reject_all`
or `continue_with_rejected`. With both extraction commands granted, review
the batch and submit its ID and hash through `batches.submit`.

For a retained scan, call `streams.extract.folder_batch_plan` with
`snapshot_id`, `snapshot_hash`, `output_root`, `selector` and
`failure_policy`. The engine derives one Matroska destination per registered
file in the existing, granted output directory. Track ordinals are resolved
independently. Inspect the excluded intake rows and child rejections before
submitting `plan.id` and `plan.hash`. The route requires
`assets.folder_inspect` and `streams.extract` in the same access profile.

Call `config.import.ffmwiz` with the UTF-8 configuration text as `source`,
an absolute `base_directory` and `compatibility_profile` set to `ffmwiz_v1`.
Review the returned diagnostics. For an admitted packet-copy configuration,
pass the returned `job` as `job_document` to `jobs.plan` with the registered
`asset_id` that matches its input. The profile needs `jobs.plan` and
`streams.clean`. This returns the ordinary reviewed plan for `jobs.submit`.

## Engine capability reports

Grant `capabilities.inspect`, `capabilities.refresh` or `capabilities.clear`
to the operating profile when needed. Inspect accepts `{"scope":"components"}`.
Refresh accepts `{"components":true,"devices":false,"force":true}`. Clear
accepts `{"namespace":"components","fingerprint":"<reported SHA-256>"}`.
All three commands use the current owner process and omit durable replay fields.
The refreshed `advertised.license` field records the exact FFmpeg executable's
recognized build notice when available.

Refresh returns the declared codecs, formats and filters separately from the
verified CPU FFV1 round trip. In an explicit `bridge --native-owner` session,
`{"params":{"devices":true,"components":false}}` returns fresh H.264 and HEVC
NVENC results for the default device.
Granting `capabilities.refresh` to an agent profile permits component refresh.
The ordinary report keeps hardware `unknown`. Clear removes the matching
in-memory component report.

## Position-based track removal

The audio and subtitle selectors accept `exclude_type_ordinals`. Each value
is a zero-based position among streams of that type in the original source.
The cleanup plan, selection report and batch plan resolve these positions for
each registered asset.

## Chapters

`chapters.inspect` takes `{"id":"<asset-id>"}` and returns exact chapter ticks,
time bases and tags from the registered probe. `chapters.export` takes
`{"asset_id":"<asset-id>"}` and returns `ffmetadata`, its SHA-256 digest and the
source content digest. Set `include_global_metadata` to `true` to include
the registered global tags. Both queries require explicit command authorization
and existing access to the asset and its source path.

`chapters.wave_cues` takes `{"id":"<asset-id>"}` and reads original RIFF WAVE
or RF64 cue data from the authorized file. It returns the sample rate and
each cue's offset, optional region length and optional label with the source
content digest. The underlying registered probe stays available through
`chapters.inspect`.

`chapters.remove` takes `{"asset_id":"<asset-id>","output":"<new-path>"}`.
It returns the ordinary reviewed cleanup plan with chapter removal enabled.
Submit that plan through `jobs.submit` to create the output.

`chapters.import` uses `asset_id`, `output` and the UTF-8 `ffmetadata` string.
It accepts up to 64 KiB and 256 ordered, nonoverlapping chapters with explicit
`TIMEBASE`, `START` and `END` fields. Use a new `.mkv` destination. Chapters
must lie inside the registered media extent and fit Matroska's nanosecond grid.
Set `include_global_metadata` to `true` to apply the document's writable
container tags and preserve unrelated tags. Review and submit the returned
plan to apply the chapter and tag changes.

## Dispositions

`disposition.patch` returns a reviewed cleanup plan. The `stream` is the
original global index of a retained audio or subtitle stream.

```json
{"params":{"asset_id":"<asset-id>","output":"<new-path>","patch":{"stream":2,"exclusive_default":true}}}
```

The patch also accepts `add`, `remove` and `clear_all`. `add` and `remove` list
FFmpeg disposition flag names. Exclusive default changes only the default bit
on peers of the selected type. Submit the exact reviewed plan via `jobs.submit`.

## Metadata changes

`metadata.patch` reviews a stream-copy plan for a new `.mkv` destination. Set
`target` to `{"scope":"global"}` for container tags or use the original stream
index in `{"scope":"stream","index":2}`. Use lowercase ASCII tag names and
UTF-8 values. The same `patch` object can be passed as the CLI patch file.

```json
{"params":{"asset_id":"<asset-id>","output":"<new-file.mkv>","patch":{"target":{"scope":"stream","index":2},"set":{"title":"Commentary"},"remove":["language"]}}}
```

Review the returned plan's metadata patch, then submit its ID and hash through
`jobs.submit`. The worker verifies retained packet and chapter data alongside
the requested metadata changes.

## Source frame preview

`media.preview_frame` takes `asset_id`, `stream_index` and zero-based `frame`.
The result contains a 160 × 90 row-major RGBA image in `rgba_base64`, the source
content digest and the selected stream/frame indices. The profile must have the
command grant and access to the registered asset and its source path.

Register a single static PNG or JPEG with `assets.register`. For either still,
`media.preview_frame` uses `stream_index: 0` and `frame: 0`.

`previews.observe` returns one standard MCP `image` block with MIME type
`image/png`. `structuredContent.result` carries the observation metadata,
including the original source digest, physical stream and decoded frame
indices, source-frame timing when available, graph and component digests,
and the delivered PNG length and digest. It requires the command grant,
the asset read grant, and access to the registered source path.

The supported parameters are `kind: "still"`, a `target` with `kind:
"registered_source"`, `asset_id`, `stream_index` and `frame_index`,
`frame_selection: "decoded_index"`, `source_policy: "original_required"`,
`display_profile: "source_monitor_rgba8"`, `fit: "contain"`, `encoding:
{"format":"png"}` and `delivery: "inline"`. This response-scoped mode
uses the 160 × 90 source monitor and caps PNG bytes at 96 KiB before base64.

To observe the same frame as the timeline monitor, set `target.kind` to
`timeline_frame` with `project_id`, `sequence_id`, `revision` and a rational
`at`. Use `frame_selection: "exact_sequence_grid"` and
`display_profile: "timeline_monitor_rgba8"` with the same PNG delivery
settings. The response identifies the sequence interval, active source or
generator, and the original source presentation time when available.
Project and source access apply to the captured revision.

For a short PCM observation, set `kind: "audio"` and provide the original
`asset_id` and `stream_index` in `target`. Set `range` to the half-open
decoded sample indices `start` and `end`. Supply the original `sample_rate`
and `channel_layout` of `mono` or `stereo`, with
`source_policy: "original_required"`, `encoding: {"format":"wave_pcm16"}`
and `delivery: "inline"`. The tool returns a standard `audio` block carrying
`audio/wav`. The structured result reports the sample grid and byte digest.
An interval above the 96 KiB inline budget returns `INLINE_BUDGET_EXCEEDED`.

`previews.source_segment` takes `asset_id`, `video_stream` and optional
`audio_stream`. It produces a manifest for the first two seconds of a zero-origin
limited BT.709 video stream with optional zero-origin stereo 48 kHz audio. The
same principal and profile may fetch the bytes through
`GET /api/v1/previews/{segment_id}/mp4` with its bearer header within 45 seconds.
The manifest supplies the initialization and ordered fragment byte ranges and a
SHA-256 digest for the complete segment. The owner's current source and command
permissions apply when serving the bytes.

`timeline.preview_frame` uses the exact project context and takes
`sequence_id`, `revision` and rational `at` in seconds. The returned RGBA frame
includes the active registered or solid-color clip identity, or a gap marker.
Choose a sequence-frame boundary. A registered source requires an exact
original video tick at the mapped time.

## Append audio and subtitles

`streams.add` accepts the typed [append request](CLI.md#append-audio-and-subtitles)
as `params`. It requires access to the original video, each additional asset
and its physical path, and a new `.mkv` destination. The review includes
`added_tracks` per readable input. Call `jobs.submit` with the returned `id`
and `hash` and the ordinary durable replay identifiers. `jobs.inspect` exposes
verification and artifact publication.

`streams.replace` uses the same execution path and accepts original-index or
type-relative audio and subtitle removals, optional additions, and an explicit
global metadata policy. A request with `metadata: "strip_global"` can omit both
track arrays. The review reports one output position for each retained or added
stream and no output position for a dropped track.

`audio.normalize` accepts `asset_id`, `output`, and one or two `tracks` with
original `stream_index` values. Each track may specify `settings` containing
`integrated_centi_lufs`, `true_peak_centi_dbtp`, and `range_centi_lu`. Review its
independent measurements through `audio_normalization` in the result, then
submit the frozen plan and inspect the completed job for verified output levels.

## Packet copy cut

`media.cut` takes `asset_id`, `output`, `chapters: "drop"`, and a `range` with
exact rational `start` and `end` in seconds. The start must match an original
keyframe on each ordinary picture stream. Packets whose original PTS is in
the half-open interval are copied in their original clock. The review returns
`packet_cut` with the accepted interval and a plan for `jobs.submit`.

## Cleanup selection reports

streams.clean.report uses the same asset_ids and full typed intent as the CLI
example. It is a query, so omit replay_scope and idempotency_key. The response
contains all candidate outcomes and bounded unique/original-index source and
retained counts. It is source inventory and a selection preview, not verification
or permission to submit a job. Current command, object and independent path rights
apply. An invisible/missing member fails the entire explicit set without a partial
report; excessive metadata returns RESOURCE_LIMIT instead of silent truncation.

streams.clean exposes the shared file_times policy: new, preserve_best_effort or
require_reviewed. Review its exact policy/snapshot and plan hash before jobs.submit.
Metadata warnings/assertions remain in jobs.inspect; private diagnostic text is
omitted from scoped views where required. Media packet clocks remain separate.

A retained stdio bridge keeps its original root and principal binding through
idle-owner restart. Reconnect never selects another root/profile principal.
Actual SDK lost-response tests withhold both accepted submission responses,
restart the owner, and replay the identical original scope/key/request to one
original receipt/job. This does not qualify retired-command or generation
migration replay.

## Direct HTTP

Run `serve --port 1578 --access-profile operator --token-file <new-file>` or the
HTTP bridge form. The exact selected loopback port hosts `/mcp` and the application
API. Configure the client's supported `Authorization: Bearer ...` facility using
the protected exported token; a token-file path is not a universal MCP option.
The file is created once and never overwritten. Current Windows ACL creation and
verification have real hosted evidence; no blanket Windows HTTP block remains.

Every request checks exact Host, optional exact Origin, origin/owner-epoch/current
audience binding, token expiry/idle limit, profile revocation and bounded admission.
Credentials have at most 8-hour lifetime and 2-hour inactive expiry. Editor lease
heartbeats do not extend idle token life. Closing/reopening a public listener
invalidates its previous audience credentials. This endpoint is stateless for both
selected lifecycle profiles; caller MCP session IDs are rejected. It does not
advertise OAuth or unrestricted remote hosting.

Browser launch uses `serve --open-browser`, the stored browser profile, a one-use
60-second proof and the separate `/api/v1/pairing/exchange` route. The proof is
removed from the URL before WASM initialization. Browser startup requires matching
registry/source assets and exact same origin. Pairing cannot create caller-selected
roots or profiles. Agent profiles cannot retain editor leases unless the owner
explicitly enabled that right in their stored profile.

## Artifact manifests and bytes

The server advertises resources without subscription or list-change promises.
resources/list uses current-authority artifact summaries with the existing
30-second cursor binding. resources/read accepts
`numera://artifact/{artifact_id}/manifest` and returns application/json text at
most 64 KiB. The manifest's media MIME is a separate field; no whole movie is
returned as a blob resource. Returned URIs and HTTP routes are lookup locations,
not permissions or credentials.

The same artifacts.manifest query is available as a typed tool. Call
artifacts.read_chunk with artifact_id, expected_sha256, decimal-string offset
and a length from 1 through 131072. Read its bounded structured result; base64 is
not duplicated into a large text block. The current profile must allow that
command and the producing job's Read action for every read. Removing job Read
also removes its exact output-byte purpose, even if the client retained a URI.
A substituted output is ARTIFACT_CHANGED even when replacement content matches.

A direct client can stream the manifest's relative bytes route on the already
running public origin using its own bearer header. Single ranges, strong digest
If-Match and no-store attachment delivery support controlled resumption. Stdio
can read chunks through its existing private bridge without starting public HTTP.
Do not transmit the private runtime record/token or put credentials in query URLs.
Observation artifact manifests, expiry, sequence-revision observations and
short audio observation require separate qualification.

## Evidence and scope

Actual CLI/SDK CPU media journeys assert byte-copy identity, remux order/tags and
same-job retry identity. Actual Linux Chrome startup exercises the embedded WASM
under enforced CSP/MIME, pairing and page teardown. This does not qualify browser
media playback, every desktop browser, native GUI interaction, the standard offline
payload, all donor commands or the owner's hardware-media routes. See the task and
continuation records for the required unfinished work.


Artifact manifests optionally include suggested_name, the authorized output
basename only. It is advisory metadata and grants no native path authority.
The existing authenticated bytes/chunk routes remain authoritative for identity,
current job rights and revocation. Native CLI clients may also use
artifacts save with the selected digest and their own local destination; this
is client delivery, not another server mutation or permission to write a path
named by an agent. No private owner bearer or query-token URL is returned.


## Repository-owned client integration

The [agent installation guide](../agents/README.md) and
[plugin package](../../integrations/numera-wiz/README.md) provide the project
skill, native client manifests and portable stdio/HTTP configurations. These
files call the Rust bridge. Manifest validation does not establish that a named
client loaded the plugin or exercised the media workflow.

`assets.register_folder` and `assets.folder_inspect` expose the same bounded
folder intake used by the CLI and shared workspace. An immutable review records
completeness, bounds, failures, companion inventories and successful media IDs.
The default discovery-only agent remains unable to register or inspect host
folders until the owner explicitly grants registration, owned-object and root
rights. Companion inventory records each file's original byte identity. A lost
mutation response is resolved with its original replay scope and key.

`streams.clean.folder_batch_plan` takes the exact retained snapshot ID and hash,
an existing output directory outside the source tree, and the ordinary cleanup
intent and policies. `output_layout: "preserve_tree"` keeps each retained
source's relative path in existing output subdirectories. Set
`copy_companions: true` to add verified copies of the inventoried files. Review
`excluded` against the snapshot and use
`batches.submit` with the returned `plan.id` and `plan.hash`. The profile needs
the folder review, folder inspection and cleanup commands plus independent read
and write grants. [Folder batch contract](../development/cleanup-batches.md#cleanup-from-a-retained-folder-snapshot)

`jobs wait` is a native CLI observer, not a new MCP tool. MCP clients poll the
existing authorized `jobs.inspect` projection and use `jobs.cancel` for accepted
job cancellation. SDK request cancellation retains its independent lifetime.


## Project context

The [project/edit/history commands](PROJECTS.md) use the same backend as CLI.
`projects.list` is a context-free query, exposed by the same registry through
stdio and HTTP. Call it with `{"params":{"limit":64}}`; omit `context.project`,
`replay_scope` and `idempotency_key`. It returns no durable receipt. Optional
`cursor` and `replace_snapshot` belong in `params` and cannot be used together;
the limit defaults to 128 and accepts 1 through 512.

The [catalog contract](PROJECTS.md#project-catalog) defines its six common-header
fields and additive authorization observations,
ascending canonical UUID order, current Read filtering, compatibility labels,
absolute 30-second continuation binding and shared quotas. Catalog replacement
can retire the exact principal/profile's own catalog or project read ticket
across projects and policy revisions; it requires current list permission and
does not require current Read on the retired project. Same-profile tabs share
handles, and paged retries have no durable exact-replay guarantee. Existing
operator profiles must deliberately grant `projects.list` using the inspected,
owner-reviewed revision-checked policy update described above, preserving their
principal/profile, roots and unrelated rights.

Read [current authorization observations](PROJECTS.md#current-authorization-observations)
as advisory captured predicates. Static descriptor requirements and the runtime
`commands.list` overlay have different scopes; neither replaces fresh request
validation. All 13 project-family tools publish `outputSchema` for the complete
canonical `structuredContent` Response. The closed envelope requires `request_id`
and `ok`: success carries its command-specific `result` without `error`; failure
carries a typed `error` without `result`. An optional `receipt_id` is a string,
never null. The error's nullable `field` is always present. Static requirements
appear under `_meta["numera-wiz/authorization_requirements"]` and contain no live
caller policy or project rights.

Discovery returns the complete standard unpaged tools list. Both its serialized
result and full JSON-RPC envelope, including the stdio newline, must fit the
262144-byte response budget. The implementation rejects an oversized or incomplete
list. Whole-Response validation has passed the actual typed CLI, official stdio
and both HTTP lifecycle journeys in
[run 43324562802](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43324562802).
That run retained the same complete 144151-byte stdio JSON-RPC line on all four
native targets; the standard list result was 144107 bytes, with 52 active tools
and 13 project output schemas. Independent artifact checks matched those emitted
schemas to all three transport validation batches. Later source changes and new
tools require fresh checks; this scoped evidence is not whole-current-CI or full-v1
qualification. See the [project contract guide](PROJECTS.md#command-contracts-and-qualification).

Source `256a98d` in [run 43336702019](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43336702019)
retained identical actual 53-tool stdio lines on all four native targets:
149128 bytes including the newline, and 149084 bytes for the standard result.
The additional tool is the native-owner read-only `maintenance.promotion.plan`.
Its emitted Result schema matches the generated schema that passed 21 checks
per target. The 13 project schemas again match all three transport batches.
Discovery does not grant owner authority; acceptance/restoration and media
execution remain unfinished. This source's formatting/unused-import failures
have follow-up corrections, so these facts do not claim complete current CI.

Source `08bff799` in [run 43352287979](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43352287979)
retained identical actual 54-tool stdio lines on all four native targets:
161759 bytes including the newline, within the unchanged 262144-byte limit.
The additional `timeline.render` Result schema matches the generated schema and
the actual public CLI validation batch. The [CI249 evidence](../development/evidence/finite-compiler-review-ci249.json)
records the four artifact hashes and schema comparison. This qualifies that
source's discovery and finite review contracts; accepted media execution and
later source changes require their own checks.

A project inspection supplies `context.project:{"id":"<project-uuid>"}`. A durable edit
also supplies the captured decimal-string `expected_revision` there, alongside
its retained `request_id`, `replay_scope` and `idempotency_key` in context:

```json
{
  "params": {
    "clip_ids": ["<clip-uuid>"],
    "delta": {"num":"1","den":"4"},
    "collision": "reject",
    "link_policy": "expand"
  },
  "context": {
    "project": {"id":"<project-uuid>","expected_revision":"5"},
    "request_id": "move-1",
    "replay_scope": "<retained-scope>",
    "idempotency_key": "move-1"
  }
}
```

Use this input for `clips.move`; creation has no project context. Queries omit
expected_revision entirely. Keep the original context on retry, even if the head
has advanced. Project Read/Edit, command permission and source-byte rights remain
separate. Existing profiles require an explicit owner policy update for the new
commands; these adapters never widen grants. Existing native/Chrome project-edit
journeys have their own qualification; mixed A/V controls, checkpoint files and
general decoded render/preview remain unfinished.

`clips.trim` uses the same project context with `clip_ids`, exact rational
`head` and `tail` amounts, and `link_policy: "expand"`. Head and tail remove time
from all linked clips. Later clips keep their sequence positions. An accepted
trim can be reverted through `history.undo`.

`timeline.render_mixed` reviews a committed range with `contract_kind:
"media_v2"`, a sequence ID, revision, range, and explicit
Matroska/FFV1/PCM download output. Pass the project ID in the tool context.
The returned plan records the original stream bindings, exact window schedule,
required reader floor and qualified FFmpeg component identity. Submit the
exact plan JSON through `jobs.submit` as `media_plan_json` with a retained
replay scope and idempotency key. The existing `jobs.inspect`,
`artifacts.manifest` and `artifacts.read_chunk` tools cover the accepted job
and its authorized output. MediaV2 maintenance requires native owner authority.

The development `timeline.render` tool uses a read context containing only the
project ID, with required exact revision in `params`:

```json
{
  "params": {
    "sequence_id": "<sequence-uuid>",
    "revision": "3",
    "range": {"start":{"num":"0","den":"1"},"end":{"num":"5","den":"2"}},
    "output": {
      "container":"matroska", "video_codec":"ffv1", "audio_codec":"pcm_s16le",
      "audio":"silence", "destination":{"kind":"download"}
    }
  },
  "context": {"project":{"id":"<project-uuid>"},"request_id":"finite-review-1"}
}
```

It requires explicit command permission and Project Read. Its static requirements
are published with the tool; the existing version1 runtime authorization
observation retains its original closed 13-command projection. The new result is
a self-contained `media_v1` review, not a rendered artifact or a legacy retained
cleanup plan. It produces no receipt. The connected development acceptance
uses the common `jobs.submit` tool with `params.media_plan_json` containing the
exact UTF-8 review string, plus the retained mutation identity in `context`.
It independently recaptures the current project before accepting immutable work.
Use ordinary job inspection/cancellation and the published media artifact's
existing manifest/chunk routes afterward. This new path still needs integrated
hosted qualification; preview and the general editor pipeline remain unfinished.
See the [finite CLI profile](CLI.md#finite-timeline-review-in-development) for
execution limits and the historical review-only qualification boundary.


Use `media.split_plan` with a registered Matroska `asset_id` and an
ordered `outputs` list of destination/range pairs. Specify
`chapters: "drop"` and grant `media.cut` alongside the split command.
Inspect each range and its outcome, then submit the plan ID and hash through
`batches.submit`. Track the individual results with `batches.inspect`.


`subtitles.burn` reviews a registered FFV1 picture, an embedded SubRip
track or registered UTF-8 SRT, the offline font, explicit audio selection,
and a source color policy. The current request profile is
`quality: "lossless_ffv1"` with a new Matroska destination. Use
`commands.schema` for the complete typed request and submit the returned
ID and hash with `jobs.submit`.


`audio.edit` reviews ordered sample intervals and per-piece reversal and
tempo for one registered PCM s16le stream. It returns exact unity-tempo
sample counts and a reviewed duration tolerance for tempo changes. It
produces one signed 16-bit WAVE stream. Use `commands.schema` to inspect
the typed input and `jobs.submit` to execute a reviewed plan.


`media.join` reviews two to eight registered compatible Matroska
inputs in `inputs_in_order` using `strategy: "copy"`. Review the
individual source durations and offsets before `jobs.submit`. Use
`audio: "drop"` for a picture-only verified copy join.


`media.retime` reviews one short FFV1 picture, an exclusive-end
source range, and tempo from 10 to 800 percent. Set `reverse: true`
to reverse the frame order and `audio: "drop"` for this picture-only
profile. The verified result retains the exact variable frame clock.


`jobs.plan` accepts a typed FFmWiz job returned by
`config.import.ffmwiz`. A job selecting FFV1, no audio or subtitle
tracks, and the supported speed/reverse settings compiles to the same
lossless picture retime as `media.retime`. Grant both commands to that
profile.


`media.encode` reviews an SDR FFV1, H.264 or HEVC source for H.264 or HEVC export
with optional AAC in MP4 or Matroska. The `settings.codec` field selects
`h264` or `hevc`. Set the preset and choose `crf` or `bitrate` in `settings`.
Use `commands.schema` for the complete contract and
`jobs.submit` to run the reviewed plan.

Set `settings.picture` to request an ordered crop, rotation, flip and resize.
Its optional `look` applies color adjustments, denoise and one detail filter
after resizing.
The job verifies the final dimensions and sample aspect alongside its original
frame timestamps.

`media.encode_folder_plan` reviews a frozen folder snapshot with the same CPU
encoder. The review lists derived destinations and excluded entries. Pass its
nested plan ID and hash to `batches.submit` and track every child through
`batches.inspect`.
