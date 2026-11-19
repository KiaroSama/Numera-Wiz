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

Portable executables default to the executable directory. The macOS app bundle
uses `~/Library/Application Support/Numera Wiz`. An explicit `--data-dir` takes
precedence and can select the existing state used by a previous development build.
Each root has protected state and runtime subdirectories and one compatible owner.
The qualified local filesystems are ext4 on Linux, APFS on macOS and NTFS on
Windows. If startup reports `RESTORATION_UNAVAILABLE`, select a root on a
supported volume with `--data-dir`.
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
| `numera-wiz doctor` | Report configured FFmpeg/ffprobe fingerprints and component readiness from the owner. |

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
| `media preview-frame --asset ID --stream INDEX --frame N` | Decode the selected video frame into a bounded 160 × 90 RGBA preview. |
| `previews observe --asset ID --stream INDEX --frame N` | Return an inline PNG from the registered source monitor with exact observation metadata. |
| `media preview-segment --asset ID --video-stream INDEX [--audio-stream INDEX]` | Prepare a temporary two-second browser preview and return its delivery manifest. |
| `timeline preview-frame --project PROJECT --sequence SEQUENCE --revision REV --at TIME` | Decode the visible picture at one exact sequence-frame boundary, with `TIME` as an integer or fraction. |
| `media capabilities inspect` | Read the FFmpeg/ffprobe fingerprints and cached component observations. |
| `media analyze-volume --asset ID --stream INDEX` | Measure peak, RMS and decoded samples per channel for one registered audio stream. |
| `media analyze-loudness --asset ID --stream INDEX` | Measure integrated loudness, true peak, range and gating for a registered audio stream. |
| `media analyze-stream --asset ID --stream INDEX --kind packets --first N --limit COUNT` | Scan the stream and read up to 64 rows of exact packet observations. Use `--kind frames` to inspect decoded video or audio frames. |
| `media compare-video --reference ID --reference-stream INDEX --candidate ID --candidate-stream INDEX --frames COUNT` | Measure PSNR and SSIM for aligned decoded video windows. |
| `media compare-audio --asset ID --stream INDEX --stream INDEX` | Compare two to sixteen original audio tracks for matching decoded samples and formats. |
| `media capabilities refresh --force` | Read advertised formats, codecs and filters and check a CPU FFV1 round trip. |
| `media capabilities refresh --devices` | Also check H.264 and HEVC NVENC on the default NVIDIA device with owner authority. |
| `media capabilities clear --fingerprint SHA` | Clear the matching component report from owner memory. |
| `chapters inspect --asset ID` | Read exact chapter ticks, time bases and tags from the registered probe. |
| `chapters wave-cues --asset ID` | Read original RIFF or RF64 cue sample offsets, recorded region lengths and labels. |
| `chapters export --asset ID` | Return UTF-8 FFmetadata and its SHA-256 digest. |
| `chapters import --asset ID --out PATH --ffmetadata CHAPTERS.txt` | Review a chapter replacement in a new Matroska file. Add `--include-global-metadata` to apply authored container tags from the document. |
| `chapters remove --asset ID --out PATH` | Review a stream-copy plan that removes chapters and preserves the retained streams and their metadata. |
| `metadata patch --asset ID --out PATH --patch PATCH.json` | Review explicit changes to container tags or to one original stream in a new Matroska file. |
| `disposition patch --asset ID --out PATH --stream INDEX --exclusive-default` | Review a plan that makes an audio or subtitle stream the only default of its type. |
| `assets register PATH --replay-scope SCOPE --idempotency-key KEY` | Register one source with durable retry identity. |
| `assets list` / `assets inspect ID` / `assets output ID` | Page assets, inspect one, or suggest a new output. |
| `streams resolve --streams-file STREAMS.json --intent-file INTENT.json` | Resolve a bounded explicit original-stream inventory. |
| `streams report --asset ID --intent-file INTENT.json` | Inspect selection counts; repeat --asset for up to 64 assets. |
| `streams clean --asset ID --out PATH --intent-file INTENT.json` | Prepare a cleanup plan for separate review and submission. |
| `streams extract --asset ID --stream INDEX --out PATH.mkv` | Review packet-copy extraction of a single original video, audio or subtitle stream. Submit the reviewed plan separately. |
| `streams extract-batch --file EXTRACTION.json` | Review up to 64 distinct Matroska outputs from registered files, including assets registered from a folder. |
| `plans inspect ID` | Inspect the exact admitted plan and its hash. |
| `jobs plan --asset ID --file IMPORT.json` | Review a versioned FFmWiz packet-copy job against its registered source. |
| `jobs submit --plan ID --plan-hash HASH --replay-scope SCOPE --idempotency-key KEY` | Submit the reviewed immutable plan. |
| `jobs inspect ID` / `jobs list` | Inspect execution or page authorized jobs. |
| `jobs cancel ID --replay-scope SCOPE --idempotency-key KEY` | Request cancellation of the selected accepted job. |
| `artifacts list` / `artifacts manifest --artifact-id ID` | Page authorized outputs or inspect one exact object. |
| `artifacts read-chunk --artifact-id ID --expected-sha256 SHA` | Read a bounded base64 chunk; optional --offset and --length. |
| `logs status` | Inspect owner log health under current owner authority. |
| `projects list/create/inspect`, `sequences create/inspect` | Discover readable projects; create and inspect durable project/sequence state. |
| `clips place/move/link/unlink/split/remove`, `history undo/redo` | Apply revision-checked project edits and retained history. |
| `markers wave-projection --project ID --clip ID --revision N` | Read original WAVE cues at their exact positions in a committed audio clip. |

See the [project command guide](PROJECTS.md) for exact fractions, expected
revisions, typed examples, source rights and bounded snapshot/history behavior.

List commands accept `--limit`, `--cursor` and `--replace-snapshot`.
`numera-wiz projects list --limit 64` discovers the complete authorized catalog
under the current 64-project store bound. It uses `projects.list`, with no project
context, replay scope, idempotency key or durable receipt. Pages default to 128
items (range 1 through 512). The six common-header fields and additive current
authorization observations, ascending UUID order,
current Read filtering, continuation binding, shared quotas and catalog-only
cross-project ticket retirement are documented in the
[project catalog contract](PROJECTS.md#project-catalog). Stored operating profiles
need a deliberate `projects.list` grant through the existing owner-reviewed
[revision-checked policy update](#authority-and-errors).

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

## Engine capability reports

Use `media capabilities refresh` to populate the owner process cache. The
fingerprint incorporates the operating system, architecture and both executable
digests. A forced refresh reads fresh lists and runs the fixed software check.
An exercised CPU round trip means one generated frame was encoded to FFV1 in
Matroska, decoded and compared with the expected samples. A failed check is
returned without caching its result.

Ordinary inspection and refresh report hardware as `unknown`. An explicit
`--devices` refresh tries each advertised NVENC encoder using one generated
frame and compares the samples returned by software decode. Results distinguish
an absent encoder, a failed route and a verified round trip. These two results
cover encoding on the default device and are measured again on every request.
Clear accepts the exact fingerprint from a report and affects only that
in-memory component entry. Restarting the owner resets the cache.

## Import FFmWiz settings

```text
numera-wiz config import-ffmwiz --input config.env --out imported-job.json
```

The command reads up to 64 KiB of UTF-8 and resolves relative media paths
against the configuration file's directory. The report contains the versioned
settings and any malformed, unknown, duplicate or unresolved assignments with
their line numbers. The shipped FFmWiz template leaves its two paths blank.
The `input_valid` field records whether the import has line or path errors.
The `--out` option creates a new report file.

The desktop and browser workspaces also have an **Import FFmWiz settings** panel
in Stream Cleanup. Paste the configuration and enter its directory on the media
engine to review the same settings and line diagnostics.

Register the media source, then use its ID to review an admitted packet-copy
configuration. The import must name that source and its new output. Set both
codecs to `copy` and choose the audio and subtitle tracks explicitly. FFmWiz
numeric track choices start at zero within each stream type.

```text
numera-wiz jobs plan --asset REGISTERED_ID --file imported-job.json
```

The planner returns the same review and semantic hash as an equivalent
`streams clean` request. Submit the reviewed ID and hash with `jobs submit`.
Missing fields and settings that require an encode or analysis profile return
a field error immediately. Import itself continues to describe all 40 fields.

## Canonical operations

`media compare-video` accepts one to eight frames. Use `--reference-start`
and `--candidate-start` for zero-based positions in the original video
streams. Choose matching dimensions, aspect ratio, color fields and frame
timing. Each decoded input window has an 8 MiB bound. The result identifies
both sources and reports per-frame PSNR and SSIM in decoded 8-bit RGB.

`media preview-frame` addresses the original stream index and a zero-based
decoded frame index. The JSON result includes the selected asset and content
digest, dimensions and `rgba_base64`. The CPU decoder fits the picture to the
preview area and fills any remaining space with black.

`media preview-segment` takes original stream indices and returns the byte
digest, MIME codec string and fragment offsets for an admitted SDR source. The
manifest expires after 45 seconds. Use the paired browser or the authenticated
same-origin delivery route described in the [MCP manual](MCP.md#source-frame-preview).

`assets register` accepts a single static PNG or JPEG of up to 32 MiB and
12,000,000 pixels. File contents determine the input format. For a registered
still, use `media preview-frame --asset ID --stream 0 --frame 0`. Animated
images and image sequences require a separate intake route.

`previews observe` selects the same original registered source stream and
zero-based decoded frame index. Its JSON result contains a 160 × 90 PNG in
`png_base64` and metadata covering the source digest, emitted frame index,
original timing when available, graph and component digests, and the PNG
byte count and digest. The source monitor fits the picture inside black
padding. Each response carries up to 96 KiB of PNG bytes before base64.

`disposition patch` addresses an original global stream index. Repeat `--add`
and `--remove` for individual FFmpeg flags such as `comment`, `original` or
`forced`. `--clear-all` clears flags on the selected stream before any additions.
`--exclusive-default` changes the default bit on that stream and its peers of
the same type. Review the resolved stream flags before submitting the plan.

### Append audio and subtitles

Register the original Matroska video and each external input separately. A
UTF-8 SubRip file up to 4 MiB can be registered directly. Save a request such as
the following and run `numera-wiz streams add --file append.json`.

```json
{
  "target": "<registered Matroska video ID>",
  "additions": [
    {"asset_id":"<external audio ID>","selector":{"kind":"all"}},
    {"asset_id":"<registered SubRip ID>","selector":{"kind":"explicit","tracks":[
      {"stream_index":0,"language":"fas","title":"Persian","disposition":["default"]}
    ]}}
  ],
  "output": "/absolute/path/new-output.mkv",
  "conversion": "forbid"
}
```

Review `added_tracks` for the source digest, original stream indices, and first
output index of each group. Submit the returned plan ID and hash through
`jobs submit`. The executor independently compares packet data, timing and codec
parameters for each retained and appended stream before publishing.

Use `streams replace --file replace.json` with the same `target`, `output` and
`conversion` fields to combine removals and additions. `remove` accepts
`{"kind":"original_index","index":2}` or
`{"kind":"type_ordinal","track_kind":"audio","ordinal":1}` for the second
audio stream. `metadata` accepts `preserve` or `strip_global`. Omit `remove`
and `additions` for a metadata-only job with `strip_global`.

### Normalize two PCM tracks independently

Run `media normalize --file normalize.json` with an authorized registered MKV,
a new `.mkv` destination, and one or two original PCM audio indices. Omitting
settings chooses −16 LUFS, −1.5 dBTP and an 11 LU range limit.

```json
{"asset_id":"ID","output":"/absolute/new.mkv","tracks":[{"stream_index":1},{"stream_index":2}]}
```

The review measures each signal and freezes its gain. On submission, FFmpeg
applies each gain to its own output track. The owner measures the staged
output again and checks its peak, loudness, original start and PCM sample
count before publication. Existing copy validation applies to unselected tracks.

### Cut by original packet boundaries

For a registered Matroska source, use
`numera-wiz media cut --asset ID --start 1/2 --end 3/2 --out /absolute/new.mkv --drop-chapters`.
The start must match an original keyframe on each ordinary picture stream.
The end is exclusive. Original packet presentation times, decode times,
durations and payloads are verified before the job publishes the output.
Audio packets may extend across the end. This command keeps the source clock
and drops chapters. Submit the reviewed plan with `jobs submit`.

### Command envelopes

The active registry contains discovery/schema, pure cleanup resolution, replay-scope
creation, access-profile create/update/inspect/put/list, file-delivered token
issue/revoke, scoped internal credential issuance, media probe, source
registration/list/inspect/output suggestion, bounded cleanup selection reports,
cleanup planning/inspection and job
submit/inspect/list/cancel, artifact list/manifest/chunk retrieval and owner log
status. `commands list` returns all active command IDs, metadata and the registry
fingerprint in one bounded response. Run `commands schema <id>` for the current
parameter schema and any published result schema.

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

Set `audio_selection` or `subtitle_selection` to remove zero-based positions
among that stream type. This example removes the second track of that type.

```json
{"selector":{"mode":"exclude_type_ordinals","values":[1]},"no_match":"reject"}
```

The positions are resolved against original stream order in each file, before
output reordering. A missing position rejects the candidate with `reject` or
skips it with `skip_file`. `allow_empty` keeps processing and records a warning.
The same selection works in a cleanup plan, selection report or batch plan.

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

The durable reader supports version-2 roots and explicitly activated version-3
generations. Older version-1 roots are refused and preserved, without automatic
reset or in-place upgrade. The owner-only copy/migration route is described below;
its retained rollback image is not an exposed rollback controller. Never delete
state to bypass a schema or ownership error.

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
Native/browser Save As streams verified client copies through the shared controls.
The complete retained-artifact lifecycle remains open; see the save instructions
below and the GUI guide for the exercised development scope.

## Extract tracks from registered files

Import a folder with `assets register-folder` and use the registered asset IDs
from its report. Give each desired output a separate row and a new `.mkv`
path in an existing destination directory.

```json
{
  "items": [
    {
      "target": {"asset_id": "registered-asset-id", "output": "/exports/audio-2.mkv"},
      "selector": {"kind": "audio", "ordinal": 2}
    },
    {
      "target": {"asset_id": "registered-asset-id", "output": "/exports/video-1.mkv"},
      "selector": {"kind": "video", "ordinal": 1}
    }
  ],
  "failure_policy": "continue_with_rejected"
}
```

Track ordinals start at one within each media type. The review records the
resolved original index for each file. A missing selector produces a rejected
row. Use `reject_all` to refuse the whole batch if a row is rejected. Submit
the reviewed ID and hash with `batches submit` and observe results with
`batches wait`. Each completed file has its own verified result.

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


## Reported remux progress

A remux reserves FFmpeg stdout for its bounded key=value progress stream:
`-progress pipe:1 -stats_period 0.2`. Process stderr remains independently
drained and bounded. The exact invocation is part of the reviewed plan and
command inspector. Older remux plans with different argv require a fresh review;
their stored plan and hash are never rewritten.

Job inspection/listing adds `remux_progress` with optional
reported_out_time_us, output_bytes and reported_frame_count, plus exact decimal
elapsed_ms/coalesced_updates and process_progress_ended. N/A stays absent.
The reported clock follows FFmpeg's progress/copyts adjustment, rather than
absolute source/sequence time. FFmpeg's first-video frame field is an observed
output counter, not a decoded-frame content oracle. No duration ratio or ETA is
invented. process_progress_ended ends the FFmpeg report stream; verification
and no-replace publication must still succeed before the job succeeds.

The incremental parser admits lines up to 4096 bytes, blocks up to 128 KiB and
1024 distinct bounded keys. It rejects duplicate/malformed known counters and
truncated/missing terminal blocks. Only the latest block is retained and the
shared job observer publishes at most one sample per 200 ms plus the final
boundary. If current authority or the observer fails, the owned process is
cancelled and drained before staging handling. The same bounded facts reach
native/browser rows, CLI and both MCP job APIs.

A tagged policy/selector/tag action rejects fields belonging to another variant,
including extra fields on disabled/all/none/keep/clear. A request cannot silently
discard the ignored part and execute the remainder. Full remux/folder/companion
and packaged five-surface qualification remains required.


## Select the source-probe deadline

Standalone `media probe` and durable `assets register` accept the optional
canonical `probe_timeout` value in their parameter object. They use the same
typed finite/disabled policy and current service admission as cleanup execution.

```text
numera-wiz media probe INPUT --probe-timeout-seconds 30
numera-wiz assets register INPUT --probe-timeout-seconds 30 --replay-scope SCOPE --idempotency-key KEY
```

Protected local-owner callers can explicitly choose --disable-probe-timeout.
Named profiles, browser sessions and scoped MCP callers receive PERMISSION_DENIED
for that choice. Absence retains the finite 120-second compatibility default.
The effective timeout is included in the returned probe, captured asset and
plan review as audit metadata. A completed source probe's timeout does not
change the semantic hash of an otherwise equivalent future execution plan.
The future cleanup execution_limits remain part of that hash. Historical
probe records omit the field and retain their earlier serialized shape/hash.

Retain the selected probe_timeout when retrying durable registration; changing
it with the same accepted scope/key is an idempotency conflict. Source-byte
identity reads, caller transport deadlines and cancellation remain independent
boundaries. This option governs the ffprobe child rather than promising a
whole-command filesystem/transport deadline. Browser upload filenames remain
opaque managed inputs; this field confers no path or object authority.


## Development data generation maintenance

`maintenance promotion plan` is a development native-owner query that reviews an
already-adopted, compatible schema3 source for MediaV1. It returns the actual
source/prefix/guard facts, current build, bounded target profile, hash and
300-second expiry. The canonical `estimated_additional_bytes` is three source-file
sizes plus a 64MiB margin, covering two independent copies and conservative COW
headroom. The available-space check does not reserve that space. The review
preserves logical source records and issues no receipt.
At source `256a98d`, shared-service tests on all four native targets passed the
actual review, literal/hash/privacy controls, browser denial and genuine
legacy/zero-adoption rejection; each target passed all 21 independent schema
checks. The current typed-CLI continuation and complete latest-source gates
still require qualification. That source's remaining native failure was an
unused import, with a separate prepared correction.
The connected development implementation also exposes explicit reviewed
`maintenance promotion accept --plan-file <file>`, `maintenance promotion inspect <id>`
and `maintenance promotion restore plan|accept|inspect` commands. The separate
`maintenance initialization` family handles eligible zero-adoption sources;
truly absent application roots use its genuine genesis path. Accept commands
require the retained `--replay-scope` and `--idempotency-key`; `--request-id` may
also be retained explicitly. Restoration plans take the accepted enablement ID.
These routes are undergoing integrated qualification. Review alone changes no
floor, and the legacy migration below retains its separate contract.

The owner-only commands `maintenance plan`, `maintenance migrate --plan-file
<raw-plan.json> --replay-scope <retained-scope> --idempotency-key <retained-key>`
and `maintenance inspect <id>` use the ordinary canonical owner interface.
Issue your scope before preparing the plan; retain the exact raw plan and retry
identity. A plan expires after300seconds and any intervening durable source
change requires a new review. Accepted jobs/intake must finish or be cancelled.

This explicit schema2-to3 path retains a verified rollback image, fences the
legacy source and activates a separate complete target. Current four-target
ordinary tests exercise same-build typed CLI migration/replay/restart and
schema3 adoption rollback on failed project acceptance. A safe rollback command
and actual prior/new packaged-binary preservation remain required. See [generation migration](../development/generation-migration.md)
for the actual admitted behavior, finite limits and remaining qualification.


## Folder intake

`assets register-folder <absolute-engine-folder>` creates an immutable intake
snapshot and registers successful media candidates. It requires the same explicit
`--replay-scope` and `--idempotency-key` as other durable mutations. Reuse the
exact command and identity after a lost response. `assets folder-inspect <id>`
reads the retained snapshot under current creator/object and root grants.

Options: `--recursive`, repeatable `--exclude-relative <normalized/relative/path>`,
`--companion-policy inventory|exclude`, and the existing explicit probe policy
flags. The default inventories companion identities; it does not copy companions
or create a media job. Exclusion paths use forward slashes, without dot segments.
Symlinks/reparse points are retained as denied outcomes and never followed.

The snapshot includes successful asset IDs, every observed failure/exclusion,
its hash, exact bounds and `complete`. A `complete: false` snapshot is incomplete
even when the command itself was durably accepted. Current bounds are 32 retained
entries, 4096 visited entries, 128 directories, depth 16, aggregate source size
4 GiB and 512 KiB directory-name storage. The scan checks 300 seconds before the
next entry; an active probe keeps its explicit policy. Existing HTTP/client request
deadlines can cancel a longer unaccepted scan; retry the original identity and
inspect any retained receipt before changing parameters. Large paged discovery
and companion/output-tree execution remain separate unfinished work.

Use returned registered asset IDs with `streams report`, `streams clean` and
explicit batch planning. For a complete snapshot with successful intake, use
`batches plan-folder --file folder-batch.json` to derive a cleanup batch for an
existing separate output directory. Review the exclusions and submit the
returned `plan.id` and `plan.hash` through the ordinary batch commands. The
[folder batch contract](../development/cleanup-batches.md#cleanup-from-a-retained-folder-snapshot)
describes the document and its limits.

## Waiting for one job

`jobs wait <id> --timeout-seconds 1800` observes the existing accepted job through
`jobs.inspect`. The deadline accepts 1 through 86400 seconds. It prints one
canonical JSON terminal response and preserves the job failure category. Failed,
interrupted, cancelled and skipped jobs are not reported as verified success.
Ctrl+C or an observation deadline stops this frontend; the owner retains the
accepted job. To cancel that job, issue the separate durable `jobs cancel` command.

## Finite timeline review in development

```console
numera-wiz timeline render --project <project-uuid> --revision 3 --sequence <sequence-uuid> --start 0 --end 5/2
```

This command reviews the exact current project revision and returns a separate
`media_v1` plan. It reads the complete bounded track/clip/semantic closure and
checks actual installed FFmpeg components. It does not produce a media file or
receipt. The connected initial media job route is undergoing integrated
qualification; exact stills and the general editor pipeline remain unfinished.
This review is separate from a retained cleanup plan.

The initial route supports one enabled video track with opaque solid corner
colors: each RGB channel is exactly 0 or 1, alpha is 1. Gaps are explicitly black;
the output intent requests silence and lossless FFV1/PCM Matroska download.
Registered sources, enabled audio tracks, effects, transparency and overlapping
layers are rejected. The current finite profile admits even dimensions through
1920x1080, rates through 120fps and ranges through 60 seconds. Boundaries must lie
on the exact frame and sample grids. No range is silently rounded or truncated.

### Mixed timeline export

The mixed export uses the selected sequence's registered streams, opaque video
layer order, solid colors and audio windows. Picture gaps render black and
uncovered audio renders silence. Exports are Matroska with FFV1 and PCM s16le.
The first profile supports fixed-size video, unit-rate clips, mono or stereo
audio and ranges of up to 600 seconds aligned to the sequence frame and sample
grid.

Native owners can review this capability with
`numera-wiz maintenance media-v2 initialization-plan` for an unadopted MediaV1
store or `numera-wiz maintenance media-v2 promotion-plan` for an adopted one.
Save the response's `result` object and pass it to `maintenance media-v2 accept
--plan-file` with a retained replay scope and idempotency key. The reviewed
source generation remains available for `maintenance media-v2 restore plan
<enablement-id>` and `restore accept` while the mixed generation remains
unadopted. Job acceptance adopts the generation.

Run `numera-wiz timeline render --mixed --project <id> --sequence <id>
--revision <revision> --start 0 --end 2` to review two seconds of a committed
sequence. Save the `result` object to a JSON file. Submit with
`numera-wiz jobs submit-media --plan-file <file> --replay-scope <scope>
--idempotency-key <key>` and retain that file for exact-byte retries.
The native and browser Projects views can review and queue their displayed
range using **Export timeline range**.

### Finite generator export

The wire query is `timeline.render` with `project:{"id":"<project-uuid>"}` and
required `sequence_id`, canonical decimal `revision`, exact `range` and explicit
`output` in `params`. New media range rationals require canonical reduced strings;
the human CLI normalizes its `numerator/denominator` input. Installed component,
project/schema, source, quota and current authority checks can reject a review.
Delegated callers need explicit `timeline.render` permission and Project Read;
Project Edit does not follow from this command. A returned frame/sample count is
a declared contract, not evidence that a file has been rendered or decoded.

To request execution in the development media generation, retain the exact
review JSON in a UTF-8 file of at most 128KiB, then use:

```console
numera-wiz jobs submit-media --plan-file review.json --replay-scope <issued-scope> --idempotency-key render-1 --request-id render-1
numera-wiz jobs wait <accepted-job-uuid> --timeout-seconds 300
```

The common `jobs.submit` request carries `media_plan_json` as the exact file
string. Keep those bytes, scope and key unchanged on retry. Acceptance checks the
current project again and freezes the reviewed payload; a later edit does not
reinterpret accepted work. Original cleanup submission keeps its existing
`plan_id`/`plan_hash` shape. Job inspection reports render, verification and
publication stages. Retrieve a published `media-<job-uuid>-output` through the
existing artifact manifest/chunk or Save As commands.

Initial execution is narrower than review: each raw decoded stream must fit
the existing 4GiB verifier bound, and video frame intervals must fit Matroska's
millisecond clock exactly. Consequently 24fps and 120fps execution are currently
rejected before acceptance. The fixed worker uses a conservative 1GiB admission
estimate; this is not an OS memory limit or measured peak. Wider clock/resource
routes remain required. These new job and generation paths have pending hosted
qualification; the historical report below covers reviews only.

The [CI249 review evidence](../development/evidence/finite-compiler-review-ci249.json)
records an actual component-backed public review, typed CLI parity and finite
result-schema checks on Linux. It also records real Store admission/reference
controls. This evidence does not qualify a rendered output, media job acceptance,
the standard offline profile or all current CI checks.
