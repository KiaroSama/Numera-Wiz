# Ordinary source handles

The current ordinary input route classifies a bounded content prefix before
probing, forces the captured demuxer, and denies automatic external dependencies.
MOV admission also walks bounded box/reference records and rejects external URL,
alias and compressed/reference modes. This does not implement concat/HLS/image
sequence/IMF dependency resolution; those remain required explicit routes.

Probe and packet verification inherit an admitted regular file on descriptor 0
using FFmpeg's seekable `fd:` protocol. The pending remux change uses the same
route: the reviewed argv names `fd:`, explicit demuxer, descriptor and block size.
It never asks FFmpeg to reopen the source pathname. The plan's source identity
and required rights still identify which native file supplies that descriptor.

At remux stage admission, the engine holds an independent physical/change stamp
for that exact opened file. It verifies the full reviewed source digest, checks
the stamp after process admission, after child completion and after full output/
packet verification. Identical-content replacement of the native pathname is a
change, not the held input. A pre-spawn rejection releases process capacity;
uncertain process termination keeps its existing quarantine contract. This is
additional to full source SHA-256 verification, never a metadata-only substitute.

A valid plan digest does not reactivate an obsolete pathname-input remux argv.
Current route validation runs at acceptance preflight and before execution;
a mismatch requires a fresh review without rewriting the historic hash.
No-op copies retain the separate Rust verified-copy route.

New hosted cases assert a real tail-indexed MOV-to-Matroska seekable remux,
existing full packet/tag/clock preservation, refusal of an obsolete route, and
replacement by identical source bytes after stage preparation with no publication
and no lost process slot. A cross-platform unit also attempts to bind a preopened
original to a new file at the old name. These changes await their own hosted
results. Original OS object identities are not yet persisted in asset records;
complete source/parent-handle leases, global output claims, reference-route
canaries and package-level race/format qualification remain required work.


The follow-up ordinary-file opener uses safe Rustix no-follow/nonblocking/close-on-exec
Unix opens and checks the opened descriptor is regular. Windows opens the final
reparse object, applies anonymous security QoS and rejects opened reparse metadata.
Hashing, stamps, ordinary admission and verified-copy sources use this boundary.
It does not yet lease every intermediate parent or interrupt all stalled filesystem
I/O. Unix FIFO and replaced-symlink regressions run only in hosted tests. New direct
OS dependency edges await the hosted resolver artifact and committed lock update.

Run 40062872192 reached the actual tail-indexed MOV remux through descriptor 0
but correctly refused publication: FFmpeg writes TrackLanguage=und and its own
Matroska demuxer omits that undefined language from metadata. The selected source
matroskaenc.c track-writing block and matroskadec.c track metadata block at commit
946fcce07b6dcd0331c8cc609192aeff5e1924f8 were inspected directly. The new frozen
verification scope admits only that exact stream language omission; known languages,
arbitrary tags and container language metadata remain exact. Focused negative cases
retain those distinctions. Old verification scopes require a fresh review rather
than inheriting this change. Full packet/clock and corruption oracles remain intact.
This follow-up also awaits hosted results.


Hosted run 40063449245 passed the full Linux engine media journey, including the
tail-indexed MOV remux, current-route and physical replacement rejections, and
existing isolated authored-tag/packet corruption negatives. Its complete actual
CLI/official SDK transport and large owner HTTP steps also passed. Ordinary
Windows and Linux tests/Clippy/desktop composition passed. The macOS FIFO fixture
required the subsequent POSIX-tool correction, and native dialog activation was
not reached in that run. Full parent leases, persisted original object IDs and
remaining global/package/reference qualification are still open.
