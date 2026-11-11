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

Per-file failed probes, exclusions, rejected files and inventory-only companions
remain visible. A partial/incomplete snapshot is not a complete folder success.
Inspect entries before selecting registered `asset_id` values and authorized
absent outputs for an explicit `streams.clean.batch_plan`. Do not submit missing
members or companions as if they were registered video assets. Batch planning,
per-member review, exact-hash submit, observation and verified delivery remain
separate operations. Repeating the original durable folder identity resolves
its snapshot receipt rather than silently rescanning changed contents.

Full donor folder/companion semantics and packaged cross-client media
qualification remain broader obligations than this bounded intake.
