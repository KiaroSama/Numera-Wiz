# Projects and timeline state — development backend

The implemented project commands store acknowledged edits in the application
database and share one service across CLI and MCP. Project controls in the shared
human workspace, portable project files, decoded preview and export remain required
work. Creating a solid-color binding here does not produce a video file.

## Commands and identity

| Canonical command | Typed CLI | Behavior |
| --- | --- | --- |
| `projects.create` | `projects create` | Create a project and root bin at revision `0`. |
| `projects.inspect` | `projects inspect` | Read summary, one entity or a bin page. |
| `sequences.create` | `sequences create` | Create explicit sequence settings and video/audio tracks. |
| `sequences.inspect` | `sequences inspect` | Read sequence detail, tracks or a bounded clip viewport. |
| `clips.place` | `clips place` | Place a registered physical stream or finite solid-color source. |
| `clips.move` | `clips move` | Move the selected/link-expanded clips by one exact delta. |
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

Queries carry `project.id` without `project.expected_revision`; omit the latter
entirely rather than setting it to null. A captured read revision belongs in
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
ID; a human project catalog is not implemented yet. Create a four-frame-per-second
sequence, retaining `created.sequence_id` and `created.tracks[0].id`:

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
whose external file is offline without granting access to its bytes.

Command permission and Project Read/Edit are separate. Native owner and browser
profiles are different principals. A browser's owned-object access does not grant
an owner-created project. The owner can explicitly add a project object grant with
`actions:["read","edit"]` through revision-checked profile update, preserving its
other grants. Project access does not add asset or filesystem rights. Existing
profiles do not automatically receive newly registered command IDs. See the
[CLI authority instructions](CLI.md#authority-and-errors).

One cooperating writer holds a user-scoped lease for each project UUID, independent
of data directory. Summary writer availability is advisory, not Edit permission.
Unknown project majors remain readable within supported projections and reject
writes; unsupported enabled semantics block their affected closure. Admitted
opaque payload text is retained; duplicate members and unsafe overlays are rejected.

## Bounded reads and history

Project inspection offers `summary`, `entity` and `bin_page`. Sequence inspection
offers `entity`, `track_page` and `viewport`. Pages default to 128/max512 and target
240 KiB inside the 256 KiB response envelope. Pass the exact cursor with the same
query fields and limit. A live ticket captures one revision for 30 seconds and
supports eight query bindings. Project and inventory tickets share 32 global/four
per-principal limits and a 16 MiB metadata/serialization reserve.

Use the same captured `--revision` for coherent projections. `--replace-snapshot`
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

The [backend evidence](../development/evidence/project-core-43056099112.json)
records four-target state regressions and the 13-job CI checkpoint. Actual
previous/new packaged-binary preservation, project GUI/checkpoints, mixed
audio/video linked variants and the complete media/editor release remain open.
