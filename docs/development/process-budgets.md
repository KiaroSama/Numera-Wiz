# Current process admission boundary

One owner has 16 admitted command requests, one scheduled heavy job and two shared
media-process slots. The latter pool is shared across every Engine clone and all
current leaf calls: metadata probe, remux and streamed source/output packet scans.
Only a leaf invocation holds a ticket; it settles before a subsequent probe or
verification stage requests another. No observer/semantic lock is held by the
gate while waiting. Admission observes cancellation and has a 120-second ceiling.
Parent adapter deadlines can cancel a request earlier. Prepared job stages also
revalidate current execution rights and source/output path policy before launch.

A successful wait/drain or confirmed cancelled/failed process returns its slot.
Uncertain tree termination, wait/capture/launch failure or an abandoned supervisor
quarantines the ticket instead. Quarantined tickets are never made available to
another process in that Engine lifetime. If both are quarantined, admission closes
and wakes waiters with RESOURCE_LIMIT. The status tuple reports configured slots,
currently available slots and quarantined slots; it is not a memory measurement.

Focused hosted unit cases exercise saturation, cancelled admission, confirmed
nonzero-exit release, uncertain reap and dropped-supervisor quarantine. They do
not themselves execute children or qualify process-tree liveness. The existing
actual process and media fixtures must continue passing. Quarantine is currently
in-memory per owner/Engine lifetime; durable cross-restart orphan accounting and
owned-process reconciliation are still required. Restart is not evidence that an
unconfirmed tree disappeared.

The two-slot gate is a process-count limit. Most FFmpeg steps use a 64 MiB
single-allocation guard. Mixed FFV1 encoding and source staging have a 256 MiB
guard and reserve that allowance in the shared acceptance pool. The FFV1
encoder can request a 178,155,184-byte worst-case packet for one BGRA HD frame.
Qualifying the target desktop working set requires separate measurements of
aggregate RSS, VRAM, scratch and child-tree memory. Cross-root destination
claims, scratch reservation, OS resource enforcement, measurements and the full
resource scheduler remain open in T017/T034/T037. Asset registration and job
submission now release the semantic lock during content hashing/probing, then
reacquire it and repeat replay/current-policy/dependency/target checks before
acceptance. Their fault cases and remaining boundaries are in preflight.md.
