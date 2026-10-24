# Two-phase durable media acceptance

Asset registration and job submission check authentication, replay scope and
receipt, explicit command/object rights and admitted paths before expensive work.
They then release the semantic mutation lock while hashing and probing. No durable
effect or receipt is committed in that phase. The existing finite request and
media-process admission bounds still apply.

On reacquisition, current principal/profile policy and the same replay scope are
checked first. A matching receipt accepted by another concurrent request wins,
even if this preflight subsequently failed or its source disappeared. That result
still passes the current restricted-receipt view. Only fresh acceptance propagates
preflight failures, after rechecking its current object/path authority. Job
submission also rechecks the retained plan payload/hash, current expiry, component
identities, exact input/output rights, destination preconditions and queue budget.
The acceptance transaction retains the submitter's current policy revision.

A SourceStamp holds a read-only file handle and physical identity across the
expensive phase and the final short acceptance. It rejects a changed canonical
path, replacement file, byte length or modification time. Full source/component
SHA-256 reads remain required during preflight. The stamp is not an immutable
content snapshot, a replacement for those digests or proof against arbitrary
in-place writes with restored metadata. Execution and verified publication still
perform their independent byte and rights checks. Source-root/directory-handle
race qualification, global destination claims and immutable managed dependency
retention remain required in T014/T017/T023; this change does not close them.

The same-file 1.0.6 primary API and Windows physical-file key implementation were
inspected. Physical identity uses its retained handle, with ordinary Rust read-only
file sharing; component/source bytes are never executed by the synthetic fault
fixtures. Hosted unit cases hold a submission at the preflight boundary and prove
that an owner policy revocation completes before it resumes, with no accepted job
or output. A second case accepts the same request concurrently, removes the source
and proves the original receipt/job wins over the delayed preflight failure. A
platform fixture replaces a source with identical bytes and rejects its changed
physical identity. These are focused acceptance/storage assertions, not full media
or filesystem-race qualification. Ordinary tests run in hosted CI only.
