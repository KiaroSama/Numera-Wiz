# MCP manual - development implementation

The adapters use official rmcp 3.6.0 and the implemented 2025-11-25 initialize and
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
using them. The stdio launcher obtains a scoped private credential; it never
forwards its native owner credential to the agent. Rotation/restart preserves the
stored profile's principal. An idle bridge alone does not retain the worker.
Use `access profiles inspect --name operator --grants-out <new-file> --json`
and revision-checked `access profiles update --grants-file <file>` to replace its
complete policy while preserving existing path/operation grants. Plan read/submit
and job read/control are separate rights. Lists use authorization-bound 30-second
summary snapshots; they do not return every full record or infer access from IDs.

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
