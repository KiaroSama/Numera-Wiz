# Bounded folder intake

Discover `assets.register_folder` and `assets.folder_inspect` in the current
registry. Registration is durable; retain its exact scope/key/params/request ID
before first sending. Inspection is a query over the immutable retained snapshot.
In addition to folder-command access, intake needs `assets.register`, creator
object authority and independent input-root rights. Existing profiles need an
explicit owner revision update to grant newly registered folder commands.

Native CLI example:

```text
numera-wiz --data-dir DATA --access-profile operator assets register-folder INPUT_ROOT --recursive --exclude-relative cache --companion-policy inventory --replay-scope SCOPE --idempotency-key folder-1
numera-wiz --data-dir DATA --access-profile operator assets folder-inspect SNAPSHOT
```

Use an explicit authorized ordinary native directory. `--recursive` is opt-in.
`--exclude-relative` is repeatable; values are normalized `/`-separated relative
paths without empty, `.` or `..` segments. An excluded path also excludes its
descendants. Exclusions are literal paths, not globs. Select the desired root
and exclusions before registration; do not silently widen traversal.

The current media filename hints are case-insensitive `.mkv`, `.mp4`, `.m4v`,
`.webm`, `.mov` and `.avi`. Each candidate still needs an admitted ordinary video
stream after the common media probe; a name alone does not prove media support.
Other regular files are companion candidates. `inventory` retains their verified
byte identity without registering a media asset or scheduling a copy; `exclude`
records their exclusion. It does not infer associations, mux attachments or
publish a companion alongside an output.

Read the returned `limits`, `complete`, `scan_error` and every entry outcome.
The current finite intake admits at most 32 manifest entries, 4096 visited
filesystem entries, 128 directories, depth 16, 4 GiB cumulative source-read
budget and a 300-second check before the next entry. An already active probe
keeps its selected finite probe policy, so the scan check is not a whole-command
hard timeout. Source registration/probe policies have the same scoped-owner
restrictions as single registration. Symlinks/reparse points and owner-managed
intake roots are rejected; protected descendant namespaces are excluded.

Inspect every snapshot entry before planning. `streams.clean.folder_batch_plan`
requires the authorized `snapshot_id`, exact `snapshot_hash`, an existing
`output_root` in a disjoint tree, and the ordinary cleanup intent, file-time,
execution-limit, failure and unresolved-index policies. The corresponding
CLI is `batches plan-folder --file folder-batch.json`. This query refuses
incomplete intake, failed entries and empty registered sets. Its `excluded`
rows refer to the retained snapshot's zero-based indices, including inventory
and explicit exclusions. Review those rows and the generated names and child
decisions in `plan`; use that exact plan ID/hash for separately authorized
submission with `batches.submit`.

For a user-selected set of individual assets, use `streams.clean.batch_plan`
with explicit destinations. Repeating the original durable folder identity
resolves its snapshot receipt. A new scan requires a new durable request.

Full donor folder/companion semantics and packaged cross-client media
qualification remain broader obligations than this bounded intake.
