# MCP manual - development implementation

The adapters use official rmcp 3.9.0 and the implemented 2025-11-25 initialize and
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

`tools/list` is generated from the active shared registry. Tool input uses
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

## Registered selection reports

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
These transfer handlers do not yet provide previews.observe image/audio content;
that separate observation and graph/revision qualification remains required.

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
rights. Companions are inventory only, not copied artifacts. A lost mutation
response is resolved with its original replay scope/key.

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
