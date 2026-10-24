# Development generation migration

This branch adds a narrow explicit schema2-to3 owner maintenance path. It is not
the full D6-J03 packaged old/new application qualification or a completed data
retention/rollback feature.

`maintenance.plan` requires an idle owner and returns a five-minute inline plan.
Issue and retain your ordinary replay scope before preparing the plan. Accepted
jobs and intake must finish or be explicitly cancelled first. Review includes
root/build/active generation, complete raw record hash/count, bytes, required
capabilities and retained rollback/target generations. A data change invalidates
the review rather than silently migrating a different snapshot.

`maintenance.migrate` is an ordinary durable mutation with the exact reviewed
plan, original replay scope and idempotency key. Preparation writes a private
manifest and complete verified schema2 backup before acceptance. Acceptance
atomically records the migration, original receipt and schema3 reader floor in
the legacy source. The target is a separate complete copy, retaining all raw
receipt bytes, exact numeric strings, IDs, policies, grants and terminal facts.
A flushed append-only activation selects that target before the same Store
attachment switches. Old generation locks remain held for the owner's lifetime.
The source's durable schema3 floor remains after it exits.

`maintenance.inspect` returns the accepted operation. The typed CLI has
`maintenance plan`, `maintenance migrate --plan-file <raw-plan.json>` with
explicit `--replay-scope`/`--idempotency-key`, and `maintenance inspect <id>`.
These use the same canonical owner endpoint. Named profiles cannot execute
maintenance, including profiles that manually list its IDs. Default operating
allowlists exclude all maintenance commands.

Recovery uses only the accepted operation. An accepted response still identifies
the same operation when the request disconnects. Startup selects the activation
index and resumes a pending migration. If recovery remains blocked, ordinary
commands/intake/credential work stay blocked and native-owner inspection remains
available; retrying the exact accepted migration request retries recovery before
returning its original receipt. All copies and errors are retained for review.
No incomplete or unrelated file is deleted as guessed ownership.

Existing files are inspected for application codec compatibility before writable
redb access or legacy profile migration. A dirty database is first copied to a
private bounded recovery probe; only that copy is repaired to determine its
version. Unsupported original stores are never repaired or initialized. A
recognized source can then undergo ordinary owner recovery. Probe retention is
bounded to8, workspace retention to16, generation size to64GiB/50000records and
per-copy directory retention to256files/128GiB, with64MiB free-space margin.

Development tests exercise ordinary Service admission and restart, accepted
migration interruptions after acceptance/copy/activation, original receipt replay,
source reader floors, raw numeric/unknown fields, full copy proof and stale-review
refusal. They require a new hosted run; no pass is inferred from source alone.

Still mandatory: safe adoption-aware rollback with retained control receipts,
actual prior/new packaged binaries and fault matrix, project journals and unsaved
edit heads, retired-version replay metadata/digest compatibility, retention roots
and collection, standard offline packages, resource budgets and owner hardware
qualification. No dummy command version is introduced to manufacture retirement
evidence. Existing schema2 legacy receipts remain opaque and exactly preserved;
their original command/principal/scope cannot be inferred from a receipt-key hash.
