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
owner creates explicit operating profiles through `access.profiles.put` before
using them. The stdio launcher obtains a scoped private credential; it never
forwards its native owner credential to the agent. Rotation/restart preserves the
stored profile's principal. An idle bridge alone does not retain the worker.

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

## Evidence and scope

Actual CLI/SDK CPU media journeys assert byte-copy identity, remux order/tags and
same-job retry identity. Actual Linux Chrome startup exercises the embedded WASM
under enforced CSP/MIME, pairing and page teardown. This does not qualify browser
media playback, every desktop browser, native GUI interaction, the standard offline
payload, all donor commands or the owner's hardware-media routes. See the task and
continuation records for the required unfinished work.
