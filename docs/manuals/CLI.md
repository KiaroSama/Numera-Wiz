# Command-line manual - development implementation

Use an identified GitHub Actions executable. Ordinary builds and tests belong in
GitHub Actions. The current desktop composition includes the shared cleanup UI;
it is not the complete editor, standard portable media package or qualified release.

## Start and attach

Global options include `--data-dir <directory>`, `--components-dir <directory>`
and `--access-profile <name>`. For CLI requests the last option selects the
effective caller; without it native CLI uses protected local-owner authority.
Stdio defaults to agent, while serve defaults to browser. In serve/HTTP bridge
the option selects the stored profile hosted by the owner-controlled listener.
The selected components directory must contain both `ffmpeg` and `ffprobe` (with
`.exe` on Windows). The current fixture payload is Linux-only development evidence,
not the required standard distribution. Processing without an admitted component
selection returns `COMPONENT_UNAVAILABLE`; discovery still works.

The default data directory is the executable's directory. The application protects
its state and runtime subdirectories and attaches to one compatible owner process.
An incompatible active binary/registry returns `OWNER_VERSION_CONFLICT`. Accepted
jobs survive a CLI/GUI/stdio client exit; an interrupted owner records recovery
facts. Never delete or replace live state to bypass a lock or version conflict.

| Invocation | Behavior |
|---|---|
| `numera-wiz commands` | Inspect the live shared registry. |
| `numera-wiz request --file request.json` | Submit one canonical request, including supported durable mutations. |
| `numera-wiz request` | Read at most 1 MiB of strict JSON from stdin. |
| `numera-wiz` | Open the human media/cleanup/job workspace in a desktop build. |
| `numera-wiz serve --open-browser` | Open the same-build browser UI with one-use pairing on exact port 1578. |
| `numera-wiz serve --port 1578 --access-profile operator --token-file <new-file>` | Retain the explicit public API/MCP listener and export a scoped token. |
| `numera-wiz bridge --access-profile operator` | Run official stdio MCP under the stored named profile. |
| `numera-wiz bridge --transport http --token-file <new-file>` | Retain direct HTTP MCP under the default discovery-only agent profile. |
| `numera-wiz doctor` | Report implementation limits without initializing media GPU or a display. |

A busy requested port returns `PORT_IN_USE`; it is never incremented silently.
The public listener drains after public client/service/work leases end; private
background jobs and a native editor can continue. Reopening the port invalidates
its earlier credentials. Owner and client requests have finite timeouts and limits.

## Ordinary composable commands

The following commands use the same canonical owner port, access profile,
strict schemas and typed JSON responses as request/MCP. Run any command with
`--help` for its options. All listed output is JSON without a separate flag.

| Command | Purpose |
|---|---|
| `commands list` / `commands schema ID` | Inspect the active registry and one parameter schema. |
| `media probe PATH` | Probe an authorized source. |
| `assets register PATH --replay-scope SCOPE --idempotency-key KEY` | Register one source with durable retry identity. |
| `assets list` / `assets inspect ID` / `assets output ID` | Page assets, inspect one, or suggest a new output. |
| `streams resolve --streams-file STREAMS.json --intent-file INTENT.json` | Resolve a bounded explicit original-stream inventory. |
| `streams report --asset ID --intent-file INTENT.json` | Inspect selection counts; repeat --asset for up to 64 assets. |
| `streams clean --asset ID --out PATH --intent-file INTENT.json` | Prepare a cleanup plan for separate review and submission. |
| `plans inspect ID` | Inspect the exact admitted plan and its hash. |
| `jobs submit --plan ID --plan-hash HASH --replay-scope SCOPE --idempotency-key KEY` | Submit the reviewed immutable plan. |
| `jobs inspect ID` / `jobs list` | Inspect execution or page authorized jobs. |
| `jobs cancel ID --replay-scope SCOPE --idempotency-key KEY` | Request cancellation of the selected accepted job. |
| `artifacts list` / `artifacts manifest --artifact-id ID` | Page authorized outputs or inspect one exact object. |
| `artifacts read-chunk --artifact-id ID --expected-sha256 SHA` | Read a bounded base64 chunk; optional --offset and --length. |
| `logs status` | Inspect owner log health under current owner authority. |

List commands accept `--limit`, `--cursor` and `--replace-snapshot`.
Cleanup planning accepts `--file-times new|preserve_best_effort|require_reviewed`.
Relative native paths resolve against the CLI process working directory before
the same canonical authorization checks. Input JSON documents are individually
limited to 1 MiB, reject duplicate keys and still share the aggregate request
budget. They are data, never executable configuration.

A practical cleanup sequence is:

1. Run `access replay-scope create --json` and retain its returned replay_scope.
2. Run `assets register INPUT --replay-scope SCOPE --idempotency-key register-1`;
   retain the returned asset ID.
3. Use [cleanup-keep-all.json](../../examples/cleanup-keep-all.json), or edit an
   explicit copy with the desired original-stream selectors and preservation.
4. Run `streams report --asset ASSET --intent-file INTENT.json`, then
   `streams clean --asset ASSET --out NEW_OUTPUT --intent-file INTENT.json`.
5. Review `plans inspect PLAN`, including source/output, selected indices,
   preservation, argv and limits. Submit its exact hash with `jobs submit`.
6. Inspect the returned job, then its artifact manifest; save a verified client
   copy when needed using the command below.

Durable typed commands require an explicit retained scope and operation key.
Without --request-id the request ID is deterministically derived from the command,
scope and key, so a separate CLI retry retains the same identity. An explicit
--request-id can be retained too. Reuse the same scope/key/parameters after a
lost response. Changed content returns IDEMPOTENCY_CONFLICT. These commands
never silently allocate a new mutation scope during a retry.

## Canonical operations

The active registry contains discovery/schema, pure cleanup resolution, replay-scope
creation, access-profile create/update/inspect/put/list, file-delivered token
issue/revoke, scoped internal credential issuance, media probe, source
registration/list/inspect/output suggestion, bounded cleanup selection reports,
cleanup planning/inspection and job
submit/inspect/list/cancel, artifact list/manifest/chunk retrieval and owner log
status. `commands.list` and `commands.schema` return generated
current parameter schemas. This list does not promise the remaining mandate scope.

A query has the following envelope:

```json
{"api_version":1,"request_id":"inspect-1","command":{"id":"media.probe","version":1},"params":{"path":"G:\\Media\\source.mkv"}}
```

First call `access.replay_scope.create` with empty params under the effective
principal. For durable `assets.register`, `jobs.submit`, `jobs.cancel` and
`access.profiles.put`, retain its returned `replay_scope` and add an
`idempotency_key` to the envelope. Retrying the same scope/key/command and params
returns the accepted receipt; changed content returns `IDEMPOTENCY_CONFLICT`.
Do not mint a new identity merely because a connection failed. An expired scope
cannot authorize another acceptance. Query/bootstrap operations reject durable
mutation fields.

Register a source with exactly one `path` or verified `upload_id`. A browser
filename is not a native path. Planning uses `streams.clean` with an `asset_id`,
explicit `output` and typed `intent`. Review its selected original stream indices,
patches, preservation flags, output, argv and verification scope. `jobs.submit`
requires that exact `plan_id` and `plan_hash`; it returns a durable `job_id`.
Inspect jobs until terminal. A no-match skip produces no output artifact. A failed
job remains a successful inspection query with failure facts in the returned job.

## Selection report and filesystem times

streams.clean.report is a query requiring current command, asset and path rights.
It accepts 1-64 distinct explicit asset IDs and the full common intent. For example:

```json
{
  "api_version": 1,
  "request_id": "selection-report-1",
  "command": {"id": "streams.clean.report", "version": 1},
  "params": {
    "asset_ids": ["<registered-asset-id>"],
    "intent": {
      "audio_selection": {"selector": {"mode": "all"}, "no_match": "skip_file"},
      "subtitle_selection": {"selector": {"mode": "all"}, "no_match": "allow_empty"},
      "preservation": {
        "additional_video": true, "attached_pictures": true,
        "attachments": true, "data": true, "unknown": true,
        "global_tags": true, "stream_tags": true,
        "chapters": true, "dispositions": true
      }
    }
  }
}
```

The report includes every resolved, skipped or rejected candidate, original
selected indices, matches, warnings and errors. Unique and original-index
summaries keep source and retained counts separate. This query does not verify
output compatibility or create a plan/job. Requests containing a missing or
invisible asset fail without a partial inventory. A changed access-policy
revision invalidates the result. At most 512 combined summary rows, 64 KiB of
retained text and a 224 KiB report are admitted; request a smaller explicit set
after RESOURCE_LIMIT. Existing profiles need an explicit owner revision update
to add this command; future registry entries never expand stored grants.

streams.clean additionally accepts file_times: new, preserve_best_effort (the
default), or require_reviewed. Required access/modification times are captured
during review and contribute to the plan hash. Best-effort failure records
FILE_TIMES_NOT_PRESERVED; required failure is FILE_TIMES_REQUIRED before the
publication fence. jobs.inspect exposes exact signed decimal-string Unix-epoch
nanoseconds and the observed assertion scope. No floating-point timestamp
conversion or source ACL/permission cloning is implied. Later reads naturally
can change access time. These policies do not alter packet timestamps.

The current durable reader envelope is version 2. Older version-1 roots are
refused and preserved, without an automatic reset or in-place upgrade. Explicit
stopped-root generation migration/rollback remains required work; do not delete
state to bypass STORE_SCHEMA_UNSUPPORTED.

## Human file workflow

Choose a native file or drag/drop it, or enter an authorized absolute engine path.
In a paired browser, selection/drop streams bytes to principal-scoped owned
staging with a 1 GiB file limit, 4 GiB aggregate managed storage limit, 8 concurrent
intakes and a finite expiry. No whole-file WASM buffer is used. Only completed,
length/digest-verified intake can be registered. The browser profile writes to the
owner's managed exports directory; a suggested destination is displayed for review.

Select audio/subtitle rules, original indices, partial ordering and Keep/Set/Clear
language/title patches. Preservation switches remain independent. Review a plan,
then explicitly run it. The queue shows actual job state and cancellation results.
The command-details inspector is supplementary to the human forms.

## Retrieve a produced output

For a successful job, its current single-output artifact ID is `job-<job-id>`.
Use artifacts.list for authorization-filtered summary pages, or request the exact
manifest:

```json
{"api_version":1,"request_id":"output-manifest","command":{"id":"artifacts.manifest","version":1},"params":{"artifact_id":"job-<job-id>"}}
```

Then explicitly request a bounded byte range using the returned SHA-256:

```json
{"api_version":1,"request_id":"output-prefix","command":{"id":"artifacts.read_chunk","version":1},"params":{"artifact_id":"job-<job-id>","expected_sha256":"<64-lowercase-hex-digits>","offset":"0","length":4096}}
```

These are queries and reject replay-scope/idempotency fields. A profile needs the
particular artifact command and current job Read. Its produced-output byte
purpose does not add general read access to the export directory. Existing
profiles need an owner revision update to admit these new command IDs.

Chunks contain base64 for the explicit requested range, at most 128 KiB raw and
256 KiB for the entire response. Only the final chunk may be short; an EOF-offset
query returns empty data and eof true. Wrong digest, substitution of the output,
out-of-bounds offsets, overflow and revoked job rights fail. Large file transfer
uses the existing listener's authenticated bytes route, single Range and strong
SHA-256 ETag/If-Match, rather than repeated media dumps into agent text. The
manifest gives a relative route, never a token. It does not start a listener.
Transfer buffers are bounded and cancellation/revocation stops subsequent frames.
Native/browser Save As controls and full retained-artifact lifecycle remain open;
the shared Output details control currently inspects verified manifest metadata.

## Authority and errors

Create an operational profile under owner authority with explicit directories:

```powershell
numera-wiz access profiles create --name operator --read-root 'G:\Media' --write-root 'G:\Exports' --json
numera-wiz access profiles inspect --name operator --grants-out 'G:\Numera Data\setup\grants.json' --json
```

The created policy has mode operate, owned_object_access true and a frozen
explicit set of the current ordinary commands. Future registry additions do not
expand it. No access-administration, device or network grant is added. Existing
names conflict. Inspection prints the stable principal, effective policy and
revision, and optionally writes a protected no-overwrite replacement grant file.
Edit that document while retaining its existing operation and path grants, then
replace the whole policy at the inspected revision:

```powershell
numera-wiz access profiles update --name operator --expected-revision 1 --grants-file 'G:\Numera Data\setup\grants.json' --json
numera-wiz --access-profile operator commands
numera-wiz bridge --access-profile operator
```

The grant document has enabled, client_sessions, mode, owned_object_access,
allowed_command_ids, object_grants, read_roots and write_roots. Name and revision
belong only in the explicit update arguments. For example, adding
`{"kind":"plan","id":"<existing-plan-id>","actions":["read","submit"]}`
to object_grants admits that plan under the recipient's own input/output rights.
Job read does not imply control. Turning owned_object_access off withdraws
inherited access to creator-owned records; separately explicit grants remain.
Opaque IDs and shared roots do not grant another principal's objects. Legacy
development access.profiles.put remains available; prefer create/update.

While the selected loopback listener is running, deliver a separate token to a
protected new file; the default audience is exact port 1578:

```powershell
numera-wiz access tokens issue --for-access-profile operator --port 1578 --out 'G:\Numera Data\setup\http-token' --json
numera-wiz access tokens revoke --token-id '<returned-token-id>' --json
```

Issue prints token ID, principal, expiry and file location, never its value.
An existing file is preserved without issuing a token. Publication failure
attempts explicit revocation and reports whether it was confirmed. Raw universal
CLI credential requests are rejected before issuance. Revocation invalidates that
credential, preserves the profile principal and leaves other credentials alone.
It does not itself revoke already accepted jobs' durable execution rights.

Asset/job list results are summary pages, default 128 and maximum 512 items with
the 256 KiB ceiling first. Pass the exact next_cursor as cursor with the same
limit. Leases last 30 seconds and bind current principal/profile revision and
projection. Refresh may supply replace_snapshot to retire only its own prior
lease. Counts are exact for authorized items. Expiry discards partial pages;
never append pages from another snapshot. assets.inspect and jobs.inspect fetch
explicit details. Plan/job views omit unauthorized paths and private invocation
internals. Revoked asset-receipt fields return RECEIPT_RESULT_RESTRICTED with
safe immutable receipt status and digest, without rerunning the operation.

Native owner authority comes from the protected runtime record. The default `agent`
profile has discovery and own replay-scope bootstrap only. Named operating profiles
must be explicitly created by the owner with their exact command, object and
canonical read/write roots. Filesystem roots do not grant another principal's
jobs or managed asset bytes. Protected state/runtime/ingest/log namespaces and
managed object output paths cannot be used as media targets.

Strict JSON rejects duplicate keys, unknown context, unsupported versions and
ambiguous source registration. Ordinary JSON request/result limits are 1 MiB and
256 KiB; binary upload routes have separate streaming limits. CLI domain errors
return `ok=false` with a stable error code and the corresponding process status.
Statuses are `0` success/accepted, `2` usage/validation, `3` availability, `4`
conflict/stale state, `5` authorization, `6` execution/verification, `7` cancellation,
`8` transport, `9` partial batch failure and `10` resource exhaustion. Inspecting
a failed job is a successful query with failure facts; it does not itself return
the failed producer job's process status. Diagnostics use timestamped
INFO, WARNING, ERROR and DEBUG on stderr; protocol stdout stays clean. The detached
owner retains protected JSONL with a 32-record queue, 16 KiB record ceiling and
five-file 2 MiB rotation. Owner `logs.status` reports health, limits and omitted/
dropped records through a canonical request. Named profiles cannot administer
these logs. Unknown reserved files are preserved for explicit repair. Actual
job/stage/publication facts remain in the store. Tokens are
never printed. Existing exported token files are preserved, including on Windows,
where the protected ACL adapter is exercised in hosted tests.

The preferred Windows location is `G:\Program Files\Numera Wiz`, with another
selected drive/path supported. Quote paths with spaces in PowerShell:

```powershell
& 'G:\Program Files\Numera Wiz\numera-wiz.exe' --data-dir 'G:\Numera Data' commands
```

Full F01-F15, complete MuxCls, editing/preview, package/resource and owner GPU
qualification remain open tasks. The owner GPU harness must eventually match the
CI-built candidate; no ordinary local source build or media test is requested.


## Save an already verified artifact

Obtain its authorized manifest, then select its exact SHA-256 and a new absolute
client destination:

```text
numera-wiz --data-dir DATA artifacts save --artifact-id job-JOB --expected-sha256 SHA256 --out ABSOLUTE_NEW_FILE
```

This streams through the same current principal's private authenticated byte
route; it does not start a public listener or another producer job. The client
destination is a user-selected local file, independent of engine path grants.
The command prints a structured verified-copy receipt. Existing entries are
preserved. Ctrl+C requests cancellation during manifest/transfer preparation or
copying and waits for admitted writes/cleanup; publication already underway is
reported by its actual outcome. See [client artifact saves](../development/client-artifact-save.md)
for limits and qualification status.


## Explicit cleanup budgets and stage progress

Cleanup requests accept `execution_limits` with two typed values:

```json
{
  "probe": {"mode":"finite","seconds":120},
  "operation": {"mode":"finite","seconds":10800}
}
```

Pass this document with `streams clean ... --limits-file LIMITS.json`.
The shared GUI exposes the same values under Execution limits. A finite value
is an integer from 1 through 604800 seconds; fractional, negative, zero,
nonfinite and unknown-field encodings are rejected. Explicit
`{"mode":"disabled"}` requires protected local-owner authority. Named profiles,
browser sessions and scoped MCP callers cannot disable a deadline. Current
admission is checked at review, submission and executable/publication boundaries.
No ambient MUXCLS environment setting alters an accepted plan.

The probe budget covers staged-output structure probing; the operation budget
covers the remux process and each full packet scan separately. A copy uses one
operation budget for writing and its two full-byte verification passes. Source
registration and preflight identity reads retain their current independent
finite/cancellation contracts; these fields do not promise a whole-job deadline.
Owned process cancellation/timeout still confirms termination/reaping with the
existing 5-second grace. A Rust copy checks between bounded filesystem calls
and drains an admitted synchronous call before returning; blocked OS I/O is
not claimed to stop immediately.

The selected policies contribute to the immutable plan hash. Historical plans
without these fields retain their serialized shape/hash and their earlier
execution interpretation; they are not silently rewritten. The default reviewed
new copy receives the finite 10800-second compatibility budget.

Job inspection and job-list rows expose the latest bounded `progress` fact for
copy_bytes, verify_staging_bytes and verify_source_bytes. completed_bytes,
total_bytes and elapsed_ms are exact decimal strings for that stage;
coalesced_updates counts suppressed repeated samples. Updates are coalesced to
one per 200 ms plus stage boundaries. A completed copy-stage byte count does
not announce verified or published output. The terminal job/artifact remains
authoritative. Remux time/ETA progress and batch aggregate accounting remain
required work.
