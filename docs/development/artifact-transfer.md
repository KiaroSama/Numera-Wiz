# Published artifact transfer

The implemented output artifact is `job-{job_id}` for the current single-output
cleanup job. This is a lookup name, not authority. `artifacts.list` uses the same
bounded, principal/policy-bound inventory snapshot kernel as job summaries.
`artifacts.manifest` and `artifacts.read_chunk` independently require their current
command permission and the producing job's Read action. General export-directory
read grants are not required and are not inferred from the job permission.

Publication retains its original hard-link proof inside the journaled private
job workspace. Recovery retains that proof after acknowledgement as well. Reads
require a successful job, matching committed publication facts, matching owned
workspace and the same physical object at the public and proof paths. A replaced
public file is ARTIFACT_CHANGED, including a replacement with identical content.
Older development jobs whose proof was already removed remain unavailable for
byte retrieval; their historical success facts are not rewritten or fabricated.

Every transfer first scans the held object, verifies the complete published SHA-256
and length and builds a temporary disk index of 128 KiB block digests. A returned
block must match the index before any of its requested bytes are returned. Reads
also recheck held physical identity, path identity, length/mtime and current job/
command policy. The index is not a substitute for the initial whole-file digest.
This also protects bytes captured during a later in-place change; new data does
not become the old artifact just because its path or length matches.

The initial scan and transfer have a finite 120 second reader deadline. The
256 GiB admitted file ceiling bounds the index to 64 MiB of temporary disk. Two
shared transfer slots bound live indexes to 128 MiB of disk. Reads allocate a
128 KiB block and at most 128 KiB of requested bytes, with one read in flight per
HTTP stream and no whole-file RAM/WASM/base64 accumulation. Cancellation and
normal drop remove the temporary index. Crash-orphan index reconciliation and
complete publication-proof/history retention/GC remain required work. The retained
hard link pins the original inode if the user removes the exported name; this
must be included in that retention accounting. No automatic history eviction is
introduced by this change.

The manifest resource is `numera://artifact/{artifact_id}/manifest` and has
application/json contents, limited to 64 KiB. Its media MIME is separately
application/octet-stream until a qualified media-specific type is established.
No native path or credential appears in the manifest. The same canonical query
backs CLI, shared native/browser Output details and both MCP transports. MCP
advertises resources without subscription/list-change promises. Explicit resource
listing uses authorization-filtered, paginated artifact summaries; resource reads
revalidate current authority and current publication bytes.

`artifacts.read_chunk` requires artifact_id, expected_sha256, canonical decimal
string offset and length from 1 through 131072. It returns offset, actual_length,
total_size, sha256, data_base64 and eof. Only a final chunk is short; offset exactly
at EOF returns an empty final chunk. Beyond EOF, overflow, stale digest, unknown
params and unbounded lengths fail. Its full result remains under the ordinary
256 KiB serialized command limit. This is explicit bounded inspection/client-
controlled transfer, not an automatic conversation dump.

When a listener is enabled, `/api/v1/artifacts/{artifact_id}/bytes` uses its existing
bearer-header authentication and current principal. The relative route in a
manifest neither starts a listener nor supplies credentials. It accepts a single
HTTP bytes range (inclusive, open-ended or suffix) and a strong digest ETag in
If-Match. Multiple/repeated ranges, invalid preconditions and query parameters are
rejected. Responses use no-store, nosniff and safe attachment delivery. Every
block is reauthorized before delivery; credential revocation, object/command
revocation, listener draining, timeout and cancellation stop later body frames.
Already delivered bytes cannot be revoked. No redirects or query bearer URLs
are introduced. The existing HTTP 130 second total request bound still applies.

New source assertions cover physical replacement, mutation, exact cross-block/
final/EOF ranges, bounds, wrong digest, cancellation, transfer capacity, restart,
write-only-export purpose and job revocation. The real owner fixture now exercises
CLI, both MCP versions/transports, manifest resources and authenticated full/range
HTTP bytes from actual produced artifacts. An isolated hosted Linux journey
transfers a 256 MiB plus 17-byte artifact under an asserted 128 MiB process RSS
ceiling. These newly added cases await their own hosted result; this is not a
claim of full B7-J03 observation, packaged download or aggregate RSS qualification.
Native Save As, browser streamed Save As, observations, retained snapshot/revision
purposes and complete artifact retention/export management remain required work.
