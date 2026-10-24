# Explicit cleanup batch development contract

This composition is under development. Its first hosted build passes WASM; the first full check found two test-only lint issues, corrected in the next revision. Actual batch media and GUI results are not yet qualified. It does not
qualify the full folder/companion/replacement workflow, editor, portable packages,
migration or owner hardware harness.

A batch contains 1-64 explicitly registered asset/output pairs and one common
cleanup intent. Every pair retains its original position. Its media plan is
created by the same planner as single-file cleanup. Index selectors mean original
global source indices, including zero. Indices absent across every eligible
audio/subtitle source are listed in the proposal; submission defaults to refusing
them unless the caller explicitly selects `per_file_no_match`.

All input policies are explicit: file times, probe/operation limits, unresolved
indices and rejected candidates. `reject_all` refuses submission when any preview
row was rejected. `continue_with_rejected` retains those rows and accepts the
remaining jobs. It does not promise atomic output publication: accepted children
use the existing individual verification and publication fences. Different
terminal outcomes are retained and never collapsed into a successful batch.

The parent hash binds the data root, input byte identities, exact reviewed
destinations, policies and each child's semantic hash or typed rejection code.
Fresh asset/plan IDs, principal creation audit and cosmetic error text do not
change execution semantics. The private parent and semantic projection are each
bounded at 224 KiB. Full child plans are loaded one at a time.

Submission has one durable receipt transaction for the group, up to 64 queued
jobs and their execution authorizations. The store samples acceptance time after
writer admission. Any late child mismatch or result budget failure rolls back
the whole acceptance. Ordinary mutation transactions retain their previous
64-record bound. Retry the same scope, key, command and parameters after an
uncertain response. Replaying an accepted receipt does not start another group.

Two batch preflight slots bound source/component hash work; existing request,
queue, process and publication budgets still apply. Scheduled cleanup jobs remain
serial under the current utility scheduler. Disconnecting a submission or a CLI
wait does not cancel an accepted worker. No whole-batch concurrency is implied.

`batches.inspect` checks current group and every child job read permission, then
rechecks the profile revision before returning its bounded summary. Large worker
diagnostic messages are omitted from group rows; inspect the authorized child
job for its full error. An unauthorized member makes the group unavailable rather
than returning a misleading partial inventory. Explicit plan/job grants may name
their composed batch-plan/batch IDs. `batches.list` uses the existing small
metadata snapshot and authorization-bound pagination; states are read live through
inspection, rather than frozen into history pages.

Cancellation checks group and every child control permission, commits queued
terminal changes and running cancellation requests with one receipt, then signals
workers. Running or verifying members are not reported stopped before they drain.
Publishing and already terminal members remain immutable. Cancellation never
removes an existing output.

Every candidate counts toward finished/total, including preflight rejection and
skip. Succeeded, failed, skipped, cancelled and interrupted totals are separate.
Successful input/output byte sums and signed output-minus-input delta include
only individually verified successful outputs. Skip/reject/cancel do not fabricate
saved bytes or output artifacts.

Canonical commands:

- `streams.clean.batch_plan`
- `batches.plan_inspect`
- `batches.submit`
- `batches.list`
- `batches.inspect`
- `batches.cancel`

Typed CLI:

```sh
numera-wiz batches plan --file batch.json
numera-wiz batches plan-inspect PLAN_ID
numera-wiz batches submit --plan PLAN_ID --plan-hash HASH --replay-scope SCOPE --idempotency-key KEY
numera-wiz batches inspect BATCH_ID
numera-wiz batches list --limit 32
numera-wiz batches cancel BATCH_ID --replay-scope SCOPE --idempotency-key CANCEL_KEY
numera-wiz batches wait BATCH_ID
```

`wait` is a client convenience, not an additional command or durable mutation.
It emits one final structured JSON response. Completed success/skips exit zero;
a completed cancellation without a published success exits 7, and mixed or failed
completion exits 9 with `PARTIAL_BATCH_FAILURE`. Ctrl+C stops observation and
leaves accepted jobs running. Querying inspection itself is successful even when
its result accurately reports a failed child.

See `examples/cleanup-batch.json` for the candidate document shape. Replace IDs
with registered assets and destinations with reviewed native paths. CLI relative
destinations are made absolute before invoking the canonical handler. A browser
must obtain engine destinations through granted output suggestions or explicitly
reviewed engine paths; its local file picker is not an engine filesystem grant.

The new records currently use the development schema-2 root. Stopped-generation
compatibility, downgrade rejection and migration remain mandatory before release.
Shared native/WASM batch controls and actual two-source native/browser input journeys
are now included as source and await their hosted execution. Media checkboxes retain
an explicit set across pages. Batch Review freezes every output/policy; Run submits
that exact proposal. Ctrl/Cmd+Enter runs the reviewed batch while its window is open.
Escape closes the batch or output-details window without cancelling accepted jobs.
The same transport retry keeps the original request identity. History and individual
job/output inspection use the canonical authorized queries. Full directory expansion,
companion policy, reviewed replacement and GUI cancellation qualification remain
required follow-up work.
No archive support row or full-task checkbox is qualified by this document.
