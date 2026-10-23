# Command-line manual - development implementation

Use an identified GitHub Actions executable. Ordinary builds and tests belong in
GitHub Actions. The current desktop composition includes the shared cleanup UI;
it is not the complete editor, standard portable media package or qualified release.

## Start and attach

Global options are `--data-dir <directory>` and `--components-dir <directory>`.
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
creation, access-profile put/list, scoped credential issuance, media probe, source
registration/list/output suggestion, cleanup planning/inspection and job
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

Native owner authority comes from the protected runtime record. The default `agent`
profile has discovery and own replay-scope bootstrap only. Named operating profiles
must be explicitly created by the owner with their exact command, object and
canonical read/write roots. Filesystem roots do not grant another principal's
jobs or managed asset bytes. Protected state/runtime/ingest/log namespaces and
managed object output paths cannot be used as media targets.

Strict JSON rejects duplicate keys, unknown context, unsupported versions and
ambiguous source registration. Ordinary JSON request/result limits are 1 MiB and
256 KiB; binary upload routes have separate streaming limits. CLI domain errors
return `ok=false`, a stable error code and nonzero exit. Diagnostics use timestamped
INFO, WARNING, ERROR and DEBUG on stderr; protocol stdout stays clean. Tokens are
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
