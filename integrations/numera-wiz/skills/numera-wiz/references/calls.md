# Calls, retries and delivery

## MCP

The server offers official Rust SDK stdio and direct Streamable HTTP adapters.
The shared registry is the source of tool IDs and parameter schemas. Host UIs
can prefix/normalize tool names; resolve the actual connected tool rather than
assuming a host-specific prefix.

Query arguments:

```json
{"params":{"id":"jobs.submit"}}
```

The example is for `commands.schema`. Discovery `commands.list` and own replay
scope bootstrap `access.replay_scope.create` each use `{"params":{}}`.

After obtaining a scope, durable `assets.register` arguments look like:

```json
{
  "params": {"path":"/absolute/authorized/source.mkv"},
  "context": {
    "request_id":"register-source-1",
    "replay_scope":"RETAIN_THE_RETURNED_SCOPE",
    "idempotency_key":"register-source-1"
  }
}
```

Replace illustrative values with the selected real values. `params` contains
only the current command's parameters; `context` accepts only `request_id`,
`replay_scope`, `idempotency_key`. Incoming params cannot change launch profile,
root or principal. Query/bootstrap calls omit durable mutation fields.

MCP domain responses use `structuredContent` with `ok`, typed result/error and
optional receipt. The short text content only points to that structured object;
reading the text alone loses the job/error. Domain failures mark the tool result
as an error. Invalid protocol envelopes use SDK protocol errors.

Durable calls are identified by the current `state_class=durable_mutation`,
including asset registration, job/batch submission and cancellation, policy
replacement, token revocation and generation migration. Retain exact command,
params, scope and key before sending. A transport cancellation/timeout can hide
an accepted receipt. Repeat the same request to recover it; changed semantics
with an accepted key cause `IDEMPOTENCY_CONFLICT`.

## CLI

For routine agent work, always specify the configured operating profile:

```text
numera-wiz --data-dir DATA --access-profile operator commands list
numera-wiz --data-dir DATA --access-profile operator commands schema streams.clean
numera-wiz --data-dir DATA --access-profile operator media probe INPUT
numera-wiz --data-dir DATA --access-profile operator access replay-scope create --json
numera-wiz --data-dir DATA --access-profile operator assets register INPUT --replay-scope SCOPE --idempotency-key register-1
numera-wiz --data-dir DATA --access-profile operator streams report --asset ASSET --intent-file INTENT.json
numera-wiz --data-dir DATA --access-profile operator streams clean --asset ASSET --out NEW_OUTPUT --intent-file INTENT.json
numera-wiz --data-dir DATA --access-profile operator plans inspect PLAN
numera-wiz --data-dir DATA --access-profile operator jobs submit --plan PLAN --plan-hash HASH --replay-scope SCOPE --idempotency-key submit-1
numera-wiz --data-dir DATA --access-profile operator jobs inspect JOB
numera-wiz --data-dir DATA --access-profile operator jobs wait JOB --timeout-seconds 1800
numera-wiz --data-dir DATA --access-profile operator artifacts manifest --artifact-id ARTIFACT
numera-wiz --data-dir DATA --access-profile operator artifacts save --artifact-id ARTIFACT --expected-sha256 SHA --out ABSOLUTE_NEW_FILE
numera-wiz --data-dir DATA --access-profile operator jobs cancel JOB --replay-scope SCOPE --idempotency-key cancel-1
```

These are individual argument-vector examples, not a shell script. Quote native
paths appropriately for the user's shell. Replace uppercase placeholders with
actual returned IDs/hashes and user-selected paths. Select `--components-dir`
consistently when needed; it contains both admitted `ffmpeg` and `ffprobe`.
Use `--help` for the identified executable. Never build another runtime adapter.

Typed mutation commands derive stable request IDs from command/scope/key when
`--request-id` is absent. If a request ID is explicit, retain it on retry too.
`request --file FILE` is the canonical fallback for commands without a shorthand;
the JSON request includes `api_version:1`, `request_id`,
`command:{"id":"COMMAND_ID","version":1}`, `params`, and durable fields only
when admitted. Strict JSON rejects duplicate/unknown keys. Request/input budget
is 1 MiB, ordinary result budget 256 KiB. Do not print raw credential responses;
use file-delivered access CLI commands.

CLI statuses: 0 success/accepted, 2 usage/validation, 3 availability,
4 conflict/stale state, 5 authorization, 6 execution/verification,
7 cancellation, 8 transport, 9 partial batch failure, 10 resource exhaustion.
Successful query status does not turn a returned failed job into success.

## Artifact bytes and HTTP limits

`numera://artifact/{artifact_id}/manifest` resources contain bounded JSON
manifests, not whole-movie blobs. `artifacts.read_chunk` returns bounded
structured base64 with exact digest and decimal offset. All reads recheck
producing-job Read rights. Client save also rechecks identity and publishes a
verified copy only to an absent destination; a suggested filename is advisory.

Direct HTTP uses the selected exact loopback origin at `/mcp` and a scoped
bearer exported to a protected new file. A client must support a private
Authorization header. Token files are not a universal MCP configuration option.
There is no SSE endpoint, OAuth service or unrestricted remote-host deployment.
Do not copy protected owner credentials, embed bearer values in repository
configs/query URLs, or forward local HTTP by opening a public port. Tokens expire,
can be revoked and are invalidated when their listener audience closes/reopens.
Stdio works without opening a public listener.
