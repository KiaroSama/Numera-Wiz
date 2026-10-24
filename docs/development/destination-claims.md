# Destination publication claims

The development publication path adds user-scoped OS file claims independent of
selected data root, executable location and caller path grants. Render and full
verification finish first. The engine then acquires all required claims, checks
target absence and the original parent again, commits the cancellation fence,
publishes with no-replace and retains the claims through the durable Published
callback. Waiting remains cancellable before that fence. Recovery obtains the
same claims before positively reconciling a publication; contention is an explicit
retryable PUBLICATION_BUSY and changes no publication fact.

A destination key includes the held native parent object plus the exact native
basename. Two fixed lock families each contain 256 stripes: entry hashes and
parent hashes. The parent stripe conservatively serializes every name in that
physical directory, covering case, Unicode normalization, short-name and other
filesystem aliases without a guessed Unicode case-fold algorithm. Different
names/directories can therefore briefly contend. This is deliberately a superset
of entry exclusion, not a per-volume name-equivalence classifier. All required
stripes are deduplicated and globally sorted before acquisition. At most 64
outputs and 128 stripe handles are admitted for one publication.

The namespace has at most 512 stripe files, one initialization guard and one
version marker. Lock files stay zero length and are never removed or truncated;
removing a locked inode could allow a second independently locked replacement.
Metadata/ACL verification does not read byte-locked file contents. An initialized
namespace must retain the exact marker. Unknown files in an uninitialized
namespace, invalid ACLs, symlinks or changed directories fail closed and are
preserved. Automatic repair of an ambiguous interrupted namespace initialization
is still required; a filename pattern alone is not deletion authority.

Unix uses a fixed /tmp user namespace keyed by effective OS UID, independent of
HOME and TMPDIR. Windows uses the current OS user's LocalAppData known folder,
independent of portable installation/data-root paths. Private directory ownership
and permissions/DACL checks remain mandatory. No application credential is stored
there. The known-folder helper is a second explicitly audited Windows FFI boundary:
it balances COM initialization, bounds the UTF-16 scan and frees the OS allocation
on success and failure. The borrowed-handle file-ID helper retains its separate
expectation. All other first-party unsafe code remains denied.

Asynchronous acquisition has a 30-second total deadline and cancellation checks.
Filesystem/ACL preparation runs outside the async executor, with eight process
slots; a timed-out or cancelled stalled OS operation retains its slot until it
returns. This prevents unlimited replacement preparation tasks. Recovery uses
nonblocking claim acquisition. Unsupported filesystem locking is explicit, not
an unprotected fallback. These bounds are not full aggregate R20 qualification.

Windows output basenames reject reserved device names (including documented
superscript device aliases), alternate-stream separators, control/reserved
characters and trailing dot/space normalization. They are never silently changed.
Normal Unicode names remain admitted. Missing reviewed parents report TARGET_CHANGED.

Pending hosted tests run two actual executable copies from different directories
with different HOME/TMPDIR/TEMP/APPDATA/LOCALAPPDATA values. One holds an absent
output claim; the other must fail for the same entry and a case/normalization
variant. Cancellation must release a waiting attempt, and killing the holder
must release the actual OS lock. The actual media engine test pauses one job at
its fence, independently proves the claim is still held, cancels another verified
attempt before its fence, then verifies the winning bytes. These tests are new
source and await CI.

This does not close T017 or qualify release packages/filesystems. Handle-relative
Unix publication, project-UUID writer leases/checkpoint overwrite rules,
interrupted-initialization repair, complete multi-output partial publication,
all crash/fence/rename/recovery journeys and package/resource evidence remain
mandatory. No complete product or GPU support follows from these claims.

Primary API contracts:
https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath
https://learn.microsoft.com/en-us/windows/win32/shell/known-folders
https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file
https://docs.rs/rustix/1.1.5/rustix/process/fn.geteuid.html
https://docs.rs/fs2/0.4.3/fs2/fn.lock_contended_error.html
