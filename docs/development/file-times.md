# Filesystem timestamps for cleanup outputs

The shared streams.clean input adds file_times with three explicit modes:
new, preserve_best_effort (the typed default), and require_reviewed. This is a
filesystem policy alongside the media intent. It does not change container tags,
packet timestamps or the copy-versus-remux decision. Both native and browser
controls send the same canonical field used by CLI and official MCP schemas.

Preserve-best-effort captures access/modification times from the held original
source after mandatory media reads/verification, applies them to verified owned
staging, and records exact desired/observed values. Unsupported metadata writes
or precision loss produce FILE_TIMES_NOT_PRESERVED warnings. The output's byte
and media assertions remain mandatory. Job summaries expose warning counts and
the actual job inspector exposes the metadata fact.

Require-reviewed freezes both source values during plan creation. Those values
and the policy contribute to the semantic plan hash. The output must retain them
exactly on staging after all content verification; failure is FILE_TIMES_REQUIRED
before the publication fence. Later source reads may update its access time;
the required target is the explicitly reviewed snapshot. New mode requests no metadata
replacement and records only the available newly generated times.

All timestamp values are signed decimal strings of Unix-epoch nanoseconds. There
is no floating-point conversion. Native clock-range/precision loss is explicit.
For managed uploads, this first policy refers to the stored local source object's
times, not unprovided original browser-file metadata. Capturing additional origin
properties remains part of managed-ingest completion.

The source handle now remains live through post-verification metadata handling
and publication. A Rust metadata stage is inspectable before its setter runs.
The publication primitive rechecks the held source after waiting for cross-root
claims and before its fence. No extra FFmpeg transform or GPU initialization is
introduced. Copy buffers retain their existing 4 MiB bound.

The metadata outcome is committed once within the job record while Verifying.
It is immutable and retained through publication/recovery. Scoped job views keep
warning codes but omit private diagnostic messages. The fact records staging's
prepublication assertion; later consumers can naturally change access time.
Creation time belongs to the new object. Ownership and permissions follow the
private owned-staging defaults; source executable bits, ACLs and extended
attributes are not cloned by this byte-copy stage.

The durable store reader envelope is version 2 so older version-1 binaries refuse
these semantic policies/facts rather than silently discarding them on a rewrite.
Version-1 roots are preserved and require the still-pending explicit stopped-root
generation migration; this checkpoint never resets or upgrades them in place.
Application, plan/project schema and store generation remain separate versions.
Historical missing plan/job fields within admitted history are omitted rather
than invented or rehashed.
A historical plan with no file_times retains its prior behavior. New plan modes
are explicit; required snapshots reject unknown fields and noncanonical numbers.
The human numeric-index parser also rejects empty CSV cells, signs, fractions,
exponents and overflow instead of silently executing a valid subset. A blank
optional order remains empty and original index 0 remains valid.

New hosted fixtures cover real native timestamp setting through verified copy
and no-replace rename on all native targets, injected denial under both policies,
exact negative-epoch conversion, unchanged new timestamps, semantic hashes and
legacy omission, independent required copy/remux timestamps, actual CLI/SDK job
facts and scoped warning-message privacy. Run 40075785336 passed these native
regressions on all four targets and the complete real Linux media/CLI/SDK and
Chrome journeys. Its formatter and one Clippy collapsible_if check failed; the
following revision applies the exact hosted format changes and simplifies that
condition. Native UI was skipped by the failed prerequisite. Scoped evidence is
in evidence/file-times-ci65.json. The ordinary shared UI has functional controls but all new control variants still
need their explicit native/browser input journeys. This does not close full F08,
copy/retention/resource/filesystem package qualification or owner GPU gates.

Primary native API:
https://doc.rust-lang.org/std/fs/struct.FileTimes.html
https://doc.rust-lang.org/std/fs/struct.File.html#method.set_times
