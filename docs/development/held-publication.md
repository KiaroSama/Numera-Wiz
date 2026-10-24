# Held-directory publication

The cleanup copy and remux publisher now adds the absent destination as an
atomic same-filesystem hard link to verified retained staging. Unix source and
destination names are single components relative to held directory descriptors;
publication does not reopen the target parent by pathname. Windows retains the
existing ordinary directory leases that exclude delete sharing and uses its
atomic no-replace hard-link primitive. Both paths reject every existing entry,
including directories and final symlinks. There is no replacing rename fallback,
cross-device copy, automatic alternate destination or post-review output rename.

The original journal-owned payload and separately journaled proof remain links
to that same object. They consume no additional content copy. Their later
collection belongs to the required retention policy. Publication synchronizes
the verified file and, on Unix, the actual held destination directory. Full
byte/media verification, current object/path authority, global destination claim,
durable fence and final durable result remain mandatory and separate.

RetainedStaging disables temporary-file name cleanup before publication
preparation, binds the original held file object to its relative staging name,
creates the proof relative to that held staging directory, and keeps both parent
leases alive. Failure after a durable fence preserves recorded paths. Proof names
cannot contain traversal or multiple path components.

A Unix directory can be renamed while its descriptor remains open. If this
happens after the final namespace check, the atomic operation still addresses
the original reviewed directory. The following namespace check returns
TARGET_CHANGED, and the attempt does not report success at the old pathname.
It preserves the proof and any actual link in the moved original directory;
it never follows a newly installed symlink into an unrelated directory. Repair
and accurate multi-artifact/interrupted-fact handling remain required. This is
not an OS guarantee against a malicious process with the same user's write
authority mutating file content or names concurrently.

New ordinary regressions exercise actual no-replace links, original-object
proofs, existing-file/directory/symlink preservation, proof traversal rejection,
and a deterministic namespace swap exactly between the final check and linkat.
The actual hosted media journey additionally observes the payload and proof
events and independently checks both objects and all bytes against its published
copy. The existing required-timestamp and full remux preservation oracles still
apply. These additions await their own hosted CI; no new test is credited yet.

Remaining T017 work includes project checkpoint writer leases, complete
interrupted namespace initialization/recovery, overwrite policies, multi-output
partial facts, fault injection at every journal/durability boundary and package/
filesystem/resource qualification. No complete utility or release gate is closed.

Primary API inspected at the admitted rustix 1.1.5 source:
https://github.com/bytecodealliance/rustix/blob/v1.1.5/src/fs/at.rs
