# Projects and timeline state — development

The implemented project commands store acknowledged edits in the application
database and share one service across CLI, MCP and the shared native/browser
workspace. Human controls provide authorized catalog discovery, project and
sequence creation, solid-color and registered-source placement, move/split/lift,
Undo/Redo, explicit stale-view Refresh and catalog reopen. Link/unlink currently
have CLI/MCP commands; their GUI controls, portable project files, full checkpoints,
decoded preview, export and the complete editor remain required work. Creating a
solid-color binding here does not produce a video file.

## Commands and identity

| Canonical command | Typed CLI | Behavior |
| --- | --- | --- |
| `projects.list` | `projects list` | Page authorized common project headers in ascending UUID order. |
| `projects.create` | `projects create` | Create a project and root bin at revision `0`. |
| `projects.inspect` | `projects inspect` | Read summary, one entity or a bin page. |
| `sequences.create` | `sequences create` | Create explicit sequence settings and video/audio tracks. |
| `sequences.inspect` | `sequences inspect` | Read sequence detail, tracks or a bounded clip viewport. |
| `clips.place` | `clips place` | Place a registered physical stream or finite solid-color source. |
| `clips.move` | `clips move` | Move the selected/link-expanded clips by one exact delta. |
| `clips.link` | `clips link` | Expand complete clip topology and assign one shared opaque group. |
| `clips.unlink` | `clips unlink` | Expand complete clip topology and clear its groups and known link references. |
| `clips.split` | `clips split` | Split the selected/link-expanded clips at an exact sequence time. |
| `clips.remove` | `clips remove` | Lift clips without moving neighboring material. |
| `history.undo` | `history undo` | Apply the retained original inverse at a new revision. |
| `history.redo` | `history redo` | Reapply the retained original at a new revision. |

All writes require a retained replay scope and idempotency key. Except creation,
they also require the project UUID and its expected decimal-string revision.
Every accepted change advances that revision. Undo restores entity identities and
content at a new revision; it does not rewind the revision counter.

After a lost response, repeat the original command, scope, key, project, expected
revision and parameters. Replay returns its recorded receipt before checking a new
revision or source. Changing the expected revision on that retry changes the
request and produces `IDEMPOTENCY_CONFLICT`. After `STALE_REVISION`, inspect the
current state and deliberately submit a new operation identity.

Project/sequence inspection queries carry `project.id` without
`project.expected_revision`; omit the latter entirely rather than setting it to
null. The catalog query has no project context. A captured read revision belongs in
`params.revision`. For example, a canonical write has this shape:

```json
{
  "api_version": 1,
  "request_id": "move-1",
  "command": {"id": "clips.move", "version": 1},
  "project": {"id": "<project-uuid>", "expected_revision": "5"},
  "replay_scope": "<retained-scope>",
  "idempotency_key": "move-1",
  "params": {
    "clip_ids": ["<clip-uuid>"],
    "delta": {"num": "1", "den": "4"},
    "collision": "reject",
    "link_policy": "expand"
  }
}
```

## A small state-editing walkthrough

Use one absolute writable `DATA` directory throughout. Replace uppercase IDs and
`SCOPE` with actual returned values; these are placeholders, not shell variables.
Keep each command and its original retry identity privately.

```text
numera-wiz --data-dir DATA access replay-scope create --json
numera-wiz --data-dir DATA projects create --name "First project" --replay-scope SCOPE --idempotency-key project-1
```

Creation returns `project_id`, `root_bin_id` and revision `"0"`. Retain the project
ID or rediscover it with `numera-wiz --data-dir DATA projects list --limit 64`.
The shared GUI also provides project and sequence creation. For this CLI
walkthrough, create a four-frame-per-second sequence, retaining
`created.sequence_id` and `created.tracks[0].id`:

```text
numera-wiz --data-dir DATA sequences create --project PROJECT --expected-revision 0 --name "Sequence" --width 64 --height 48 --frame-rate 4/1 --sample-rate 48000 --audio-layout stereo --video-track Video --replay-scope SCOPE --idempotency-key sequence-1
numera-wiz --data-dir DATA clips place --project PROJECT --expected-revision 1 --sequence SEQUENCE --track TRACK --rgba 0,0,0,1 --source-duration 4/1 --source-start 0/1 --source-end 4/1 --sequence-start 0/1 --replay-scope SCOPE --idempotency-key place-1
```

Placement returns revision `"2"`, `created.source_id` and `created.clip_ids[0]`.
Call that clip `LEFT`. Split at two seconds; retain the returned right clip as
`RIGHT`. The left ID remains unchanged:

```text
numera-wiz --data-dir DATA clips split --project PROJECT --expected-revision 2 --clip LEFT --at 2/1 --replay-scope SCOPE --idempotency-key split-1
numera-wiz --data-dir DATA history undo --project PROJECT --expected-revision 3 --replay-scope SCOPE --idempotency-key undo-1
numera-wiz --data-dir DATA history redo --project PROJECT --expected-revision 4 --replay-scope SCOPE --idempotency-key redo-1
numera-wiz --data-dir DATA clips move --project PROJECT --expected-revision 5 --clip LEFT --clip RIGHT --delta 1/1 --replay-scope SCOPE --idempotency-key move-1
numera-wiz --data-dir DATA clips remove --project PROJECT --expected-revision 6 --clip RIGHT --replay-scope SCOPE --idempotency-key lift-1
numera-wiz --data-dir DATA projects inspect --project PROJECT
numera-wiz --data-dir DATA sequences inspect --project PROJECT --sequence SEQUENCE --projection viewport --track TRACK --start 0/1 --end 6/1 --limit 128
```

With no competing edits, the final revision is `"7"`. The remaining left clip
occupies `[1,3)` seconds; lift leaves its neighbors in place. Query a clip's entity
projection for its exact source range. A zero-delta move or supported boundary
split returns an accepted `no_change:true` receipt without changing revision,
entity bytes, history or redo. Locks, grids and compatibility still apply.

## Link and unlink topology

`clips.link` and `clips.unlink` use the same canonical service through typed CLI
and MCP. Both accept 1–128 distinct seed `clip_ids` and require
`link_policy:"expand"`; the typed CLI supplies that policy. Repeated `--clip`
flags select seeds. Duplicate seeds reject. Expansion includes complete group
membership and both incoming and outgoing direct clip links, up to 128 resolved
clips, all in the same sequence. Existing locks, compatibility, frozen binding
validation and edit budgets still apply before any no-change acceptance.

Link requires at least two resolved clips, even when one seed reaches the whole
component. It assigns one fresh collision-checked opaque `link_group_id`, retaining
direct links, geometry and source bindings. The ID is not a Group entity. If all
resolved clips already share one non-null group, Link returns that same ID as a
validated no-change without allocating another ID. Unlink needs at least one
resolved clip, expands the whole component, and clears only group values and known
`role:"link"` references. It allocates no IDs; already unlinked members retain
their raw bytes.

The following is an illustration, not a recorded CLI execution, and is independent
of the preceding walkthrough. Starting state: project `PROJECT` is at revision
`"8"`, with distinct unlinked clips `CLIP_A` and `CLIP_B` in one sequence, no direct
links and no other group members. `DATA`, `PROJECT`, `CLIP_A`, `CLIP_B` and `SCOPE`
are placeholders; replace them with an absolute data directory, returned IDs and
a privately retained replay scope. The operation keys/request IDs below are
illustrative, fresh identities in that scope.

```text
numera-wiz --data-dir DATA clips link --project PROJECT --expected-revision 8 --clip CLIP_A --clip CLIP_B --replay-scope SCOPE --idempotency-key topology-link-1 --request-id topology-link-1
numera-wiz --data-dir DATA clips link --project PROJECT --expected-revision 9 --clip CLIP_A --replay-scope SCOPE --idempotency-key topology-link-noop-1 --request-id topology-link-noop-1
numera-wiz --data-dir DATA clips unlink --project PROJECT --expected-revision 9 --clip CLIP_A --clip CLIP_B --replay-scope SCOPE --idempotency-key topology-unlink-1 --request-id topology-unlink-1
numera-wiz --data-dir DATA history undo --project PROJECT --expected-revision 10 --replay-scope SCOPE --idempotency-key topology-undo-1 --request-id topology-undo-1
numera-wiz --data-dir DATA history redo --project PROJECT --expected-revision 11 --replay-scope SCOPE --idempotency-key topology-redo-1 --request-id topology-redo-1
```

With no competing edits, Link advances to `"9"`; the second Link is no-change at
`"9"`. Unlink advances to `"10"`, Undo to `"11"` and Redo to `"12"`. Undo restores
the same group ID and retained entity images; Redo reapplies the stored images,
without generating a replacement group.

Link's `created` value is exactly `{link_group_id, clip_ids}`; Unlink's is exactly
`{clip_ids}`. In each, `clip_ids` is the sorted complete resolved selection,
including unchanged members. These fields describe the resolved topology;
`changed_ids` contains only actual changes. A validated no-change returns an
accepted, replayable receipt with `no_change:true` and empty `changed_ids`, while
revision, raw project/entity bytes, history and redo remain unchanged. Retrying a
lost response uses the exact original identity, expected revision and parameters,
including for no-change; it is different from the fresh second Link above.

## Sources, time and authorization

Times use exact integer fractions. Video positions must lie on the declared frame
grid and audio positions on the sample grid. Registered source spans may include
negative source PTS; timeline positions stay nonnegative. Current mappings are
forward unit-rate and reject collisions. Existing explicit/incoming/group links
expand together; a linked interior split must be interior to every member.

For registered media, replace `--rgba`/`--source-duration` with
`--asset ASSET --stream-index INDEX`. Use the physical original stream index,
including valid index `0`. Registration and placement require the current asset
and native source rights. The binding captures asset ID, content/probe digests,
physical index and media kind; `extent_verification:"pending"` is not decoded
coverage proof. Project reads expose no copied native source path or full probe.
Move/lift/history operate on retained project facts; undo can restore a binding
whose external file is offline without granting access to its bytes. Both Link and
Unlink are topology-only edits over frozen admitted binding facts. They require
current command permission and Project Edit at acceptance, but do not open source
bytes, add a fresh Asset Read/path-grant check or establish current-byte validity
or decoded extent. Registered placement and media-consuming operations retain
their current source rights and held-source preflight requirements.

Command permission and Project Read/Edit are separate. Native owner and browser
profiles are different principals. A browser's owned-object access does not grant
an owner-created project. The owner can explicitly add a project object grant with
`actions:["read","edit"]` through revision-checked profile update, preserving its
other grants. Project access does not add asset or filesystem rights. Existing
profiles do not automatically receive newly registered command IDs. The owner
must explicitly review and add `clips.link` and `clips.unlink` to a stored profile's
command allowlist while preserving its unrelated grants; discovery never widens
it. Project Edit grants no filesystem authority. See the
[CLI authority instructions](CLI.md#authority-and-errors).

One cooperating writer holds a user-scoped lease for each project UUID, independent
of data directory. Summary writer availability is advisory, not Edit permission.
Unknown project majors remain readable within supported projections and reject
writes; unsupported enabled semantics block their affected closure. Admitted
opaque payload text is retained; duplicate members and unsafe overlays are rejected.

## Project catalog

`numera-wiz projects list` lowers to the canonical `projects.list` query used by
MCP. It accepts `--limit` (default 128, range 1 through 512), `--cursor` and
`--replace-snapshot`. It takes no ProjectRef, replay scope or idempotency key and
returns no durable receipt. For example, a canonical request is:

```json
{
  "api_version": 1,
  "request_id": "catalog-1",
  "command": {"id": "projects.list", "version": 1},
  "params": {"limit": 64}
}
```

Each item retains `id`, `name`, decimal-string `revision`, `schema_major`,
`envelope_version` and `compatibility`, and adds `authorization` as described below.
Items are ordered by ascending canonical
project UUID (`id`); there is no arbitrary sorting or recency order. Compatibility
is `supported` or `read_only_unsupported`, describing common schema/envelope
readability. Unsupported common headers can remain visible without promising
editing support. Effective authority, enabled semantics and writer availability
have their own checks. The catalog reads headers, not entities, history, source
paths or writer leases. Its outer `project_revision` and `event_cursor` are null;
row revisions are independent captured facts.

Current Project Read rights filter rows before items and exact counts are
computed. The current store limit is 64 projects, so `--limit 64` returns the
complete authorized catalog in one unretained page. Smaller pages can retain a
continuation bound to principal, profile, policy revision, projection and limit
for an absolute 30 seconds. Pass the exact `next_cursor` with the same limit;
expiry or a policy change requires a fresh query. Paged queries are ordinary
queries without a durable exact-replay guarantee: a lost-response retry can
allocate another retained snapshot, subject to quota and expiry.

For this catalog command alone, `--replace-snapshot SNAPSHOT_ID` may retire an
owned catalog snapshot or project read ticket, including one for another project
or an older policy revision. Ownership requires the exact principal and profile;
native owner has no bypass for another caller's ticket. Current `projects.list`
permission is required, but current Read on the old project is not. The returned
catalog is always filtered under current rights. Same-profile tabs share this
ownership boundary and can expire each other's handles. Cursor and replacement
cannot be supplied together. Unknown, foreign and expired replacement tokens
return no existence information; removing a real owned ticket can legitimately
free quota. Other inventories and inspect replacement behavior are unchanged.

Catalog and project tickets share four retained snapshots per principal, 32
globally and a 16 MiB metadata/serialization reserve. Results remain bounded to
240 KiB inside the 256 KiB response envelope. An unretained catalog does not need
a count slot, even at capacity; it still undergoes byte-budget checks.

Stored operating profiles must deliberately add `projects.list` to their command
allowlist. Use the existing [owner-reviewed grant update](CLI.md#authority-and-errors):
inspect with `--grants-out`, preserve principal/profile, roots and unrelated rights,
then update with the captured `--expected-revision`. Discovery and adapters never
silently widen grants. Command permission and Project Read remain separate.

## Current authorization observations

Catalog rows and every project/sequence inspection result include an additive
`authorization` object with `schema_version:1`. It contains canonical decimal
strings `policy_revision` and `observed_project_revision`, independent
`current_read` and `current_edit` observations, and 11 project-target `actions`.
Each right/action is either `unavailable` or `requires_request_validation`.
The latter means that the observed predicates pass; it never means an edit is
ready or already authorized. Each action names its exact command and reports
finite `reasons` and `unchecked_gates`.

Command allowance, Read and Edit remain separate. A Read-only profile can see
Link as unavailable for `project_edit_not_granted` even when `clips.link` is
allowed. Granting Edit does not add `clips.link` to its command allowlist.
Unsupported headers or an already-observed busy writer can block write actions
without changing the independent Edit observation. Summary reuses its existing
writer observation; catalog and other projections do not probe writer leases.
Even an available writer leaves `writer_lease` unchecked. Source rights/path,
revision, selection/affected scope and history are validated by the actual fresh
request where applicable, rather than by catalog discovery.

One captured policy filters and annotates a page. A policy change before
publication rejects the response instead of publishing mixed old/new rights.
Retained content remains at its captured `project_revision`; its authorization
can observe a newer `observed_project_revision`. Missing metadata or an unknown
authorization schema version means unknown authority. Clients must not derive an
Edit grant from legacy `read_only`, compatibility or writer-status fields.

The 13 project-family descriptors also publish static
`authorization_requirements`. These describe project action and request gates,
without caller facts. Authenticated runtime `commands.list` adds a separate
13-action `authorization` overlay that observes exact command allowance only;
without a project context, Read/Edit remain unchecked. `commands.schema` stays
a direct static descriptor. Pure static discovery and MCP tool metadata never
manufacture owner authority. Live observations do not enter the static registry
fingerprint.

## Command contracts and qualification

The command table above lists 13 implemented project-family handlers.
All 13 publish `result_schema` through `descriptor_schema_version:2`; historical
descriptors without that additive member remain readable. Result documents are
self-contained and describe the emitted result, including flattened entity
payloads, explicit nullable fields and omitted no-change flags.
[Run 43288625608](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43288625608)
validated six service batches on all four native targets plus actual typed CLI,
official stdio and both HTTP transport journeys with a separately locked JSON
Schema validator. Its whole CI run was not green; these are scoped result-contract
observations before the newer authorization/input changes.

Input descriptors declare finite projection/sort/edit-policy values, query-specific
required fields, page/selection bounds and explicit sequence settings. They retain
existing optional nulls and defaults. JSON Schema character lengths do not replace
runtime UTF-8 byte bounds, and exact time/grid/closure/source validation remains
runtime work. Noncanonical accepted input fraction strings are not normalized in
the original request or its retry digest.

[Run 43315700376](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43315700376)
validated 94 independent literal input cases and 34 authorization-output cases
against the 13 published schemas on each of the four native targets. Existing
parser, public-profile and final-publication race assertions passed in the shared
service tests. These are scoped observations; the whole run later failed at an
unrelated MCP test-module path. Later source changes still require current checks.

The MCP tools now publish whole-Response output schemas. Actual CLI, official
stdio and both HTTP lifecycle responses passed their independent schema batches
in [run 43319569511](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43319569511).
The same run retained and independently checked actual stdio frames on all four
native targets: a complete 52-tool standard list result of 144107 bytes and its
144151-byte JSON-RPC line, including the newline. All 13 project output-schema
hashes matched the actual CLI, stdio and HTTP validation documents. This evidence
is for source `438b06c`; the run still failed at the new media-promotion regression
and formatting. Later source changes require fresh checks, and full v1 remains
unfinished.

## Bounded reads and history

Project inspection offers `summary`, `entity` and `bin_page`. Sequence inspection
offers `entity`, `track_page` and `viewport`. Pages default to 128/max512 and target
240 KiB inside the 256 KiB response envelope. Pass the exact cursor with the same
query fields and limit. A live ticket captures one revision for 30 seconds and
supports eight query bindings. Project and inventory tickets share 32 global/four
per-principal limits and a 16 MiB metadata/serialization reserve.

Use the same captured `--revision` for coherent projections. For project/sequence
inspection, `--replace-snapshot`
can retire an owned same-project ticket before a fresh capture. Expired cursors
require an explicit refresh; a revision without a live ticket is not reconstructed
from the latest head. Observed file growth and conservative write estimates can
expire tickets early; they are not hard instantaneous disk or RSS limits.

Edits admit at most 128 entity changes, 512 index/reference writes and a 128 KiB
original forward/inverse patch pair. History retains at most 256 originals/32 MiB
and 128 distinct source identities; complete oldest originals may be evicted earlier
to keep branch edits bounded. New edits clear redo atomically. Live bindings remain
referenced independently of retained history. The operation journal has its own
512-record/8 MiB bound, reported separately from the history floor.

The historical [backend evidence](../development/evidence/project-core-43056099112.json)
records four-target state regressions and that source's 13-job CI checkpoint.
Actual prior/current packaged-binary preservation has qualified the unlinked
project scenario and the positive group-only Link/Move/Unlink/Undo fixture within
their exercised scope. In run 43288625608, the pinned old/current packages passed
seven owner epochs on each native target, preserving the same group, two exact
dependency rows, eight receipts and original raw/rollback checks. This does not
qualify mixed audio/video, registered sources or other dependency roles.
Shared native/Chrome project input journeys
passed in [run 43232250861](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43232250861)
for the controls described above. That run also reached the integrated Link/Unlink
assertions on all four native targets before a separate later Split failure;
[run 43237451380](https://github.com/KiaroSama/Numera-Wiz/actions/runs/43237451380)
subsequently exercised the complete focused Split model on all four targets.
These are scoped observations, not claims that those whole runs or current CI
are green. Linked GUI journeys, mixed audio/video and media variants,
portable project files/full checkpoints,
decoded preview/export, named external agent runtimes and the complete
media/editor release remain open.
