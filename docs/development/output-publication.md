# Reviewed output directories and publication

New cleanup plans retain the original native output-directory object in
required_rights.output_create.parent_object, alongside the canonical destination
and no-replace policy. This identity is semantic and therefore changes the plan
hash if a different directory later occupies the same pathname. Child entries
and directory modification time are not directory identity. The key uses the
same Unix device/inode/birth-time or full Windows volume/128-bit ID contract as
registered inputs. It is independent of current profile revision and review
metadata; current object and path authority still require separate checks.

DirectoryStamp opens an ordinary directory without following its final link or
reparse point, retains the handle and rechecks its canonical pathname against the
held original object. Windows excludes delete sharing while that lease is live.
Unix detects a renamed/replaced pathname; an open directory handle alone does
not prevent namespace changes there. Full byte verification remains independent.

Planning captures the directory object after resolving an absent no-replace
target. Job preflight holds the original directory across expensive source and
component checks and validates it again under fresh acceptance authority. Engine
execution retains it across copy/remux; staging and final publication independently
bind the current directory to the frozen key. Fence/publication and recovery
checks reject a replacement even if the canonical pathname and destination
absence are unchanged. A recovery lacking original directory identity cannot
infer successful publication into whichever directory now occupies that path.

The optional field defaults to absent and is omitted when absent. Historical
raw JSON and hashes are not silently rewritten. Inspection and terminal records
remain historical; a new execution requires a fresh review with the parent key.
This does not silently upgrade old acceptance into a different destination.

New hosted fixtures cover parent replacement after review, a changed parent
while Unix preflight is paused outside the semantic lock, child changes retaining
identity, live Windows namespace exclusion and the actual FFmpeg fixture's fresh
review/copy into the replacement directory. These source additions await CI.

This checkpoint does not complete T017. Publication still needs the secure,
user-scoped, data-root-independent destination claims mandated in the ownership
handoff, deterministic multi-output acquisition, cancellable claim waiting,
handle-relative operations and all required crash/fence/rename fault journeys.
Claims must begin only after render/verification, cover final target checks,
fence/publication and the durable result, and be reacquired during recovery.
They must account for absent targets and filesystem case/normalization aliases;
a per-data-root lock or a lock retained for the whole render is insufficient.
Project checkpoint replacement additionally requires the target project's writer
lease. The current media route remains atomic no-replace only; no overwrite or
project-save capability is inferred from these directory identity checks.
