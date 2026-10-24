# Native owner reattachment

A native adapter binds its selected root UUID for its lifetime, independently of
the cached private port/epoch/owner credential. Active health must match the
protected bootstrap epoch, root, build and registry. Clearing a stale connection
cache does not clear the root binding. A different data identity requires a new
explicit adapter session and returns OWNER_IDENTITY_CONFLICT to the old session.

A scoped adapter also binds the profile's principal after initial bootstrap.
Later credential issuance includes expected_principal. The owner compares that
expectation before issuing or retaining another credential; a profile name reused
for another principal returns PRINCIPAL_CHANGED. The client independently checks
the returned profile/principal and keeps its original binding on failure. The
owner credential remains bootstrap-only; it is never used for agent commands.

Normal idle owner shutdown does not close stdio framing. The next request can
reattach/start the matching build and replace stale port/epoch credentials under
the same root and principal. A failed bootstrap transport can refresh the stale
attachment once; policy or principal rejection does not trigger a wider identity.
Durable request bodies, replay scopes and idempotency keys are retained verbatim
across that retry. Current profile/object/path rights remain independently checked.

Native bootstrap DomainError results are preserved as bounded canonical MCP
command errors with the original request ID. Principal changes are conflicts
(CLI status 4). Unclassified attachment failures still use the existing bounded
transport failure and do not become a fresh durable mutation.

The new hosted SDK regression keeps one real stdio process alive through two
idle owner drains. It compares actual server-issued scope root/principal values
and protected owner epochs, then replaces a profile principal only while no owner
or store writer is alive. The original bridge must reject reattachment; a newly
selected bridge can bind the new profile identity. A focused service test checks
that the rejection creates no additional credential. A binding unit rejects root
replacement. These additions await their own CI; no root-replacement package,
upgrade/rollback, full owner-death recovery or release qualification is inferred.


The pending startup correction holds a protected runtime-directory OS bootstrap
gate across launch/readiness and allows at most one child per attachment attempt.
Other frontends continue checking the authenticated health record while that gate
is occupied. The separate store writer lock remains authoritative. This prevents
one-second retries from launching duplicate initializers before the original
process reaches its store lock; no startup deadline or identity check is relaxed.
A new real three-frontend/same-root test asserts successful commands and exactly
one observed child-spawn event across all frontend stderr streams and one
structured owner initialization PID, then waits for the idle writer drain.
Its evidence is pending.
