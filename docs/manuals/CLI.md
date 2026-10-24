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

## Canonical operations

The active registry contains discovery/schema, pure cleanup resolution, replay-scope
creation, access-profile create/update/inspect/put/list, file-delivered token
issue/revoke, scoped internal credential issuance, media probe, source
registration/list/inspect/output suggestion, cleanup planning/inspection and job
submit/inspect/list/cancel. `commands.list` and `commands.schema` return generated
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
