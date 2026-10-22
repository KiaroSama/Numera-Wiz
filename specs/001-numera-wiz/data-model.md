# Data model

Exact rational timestamps use lossless decimal-string numerator/positive denominator.
A stream has absolute source index, kind, codec, complete observed tags and disposition
flags. Selectors are tagged all/none/indices/languages/title variants with explicit
skip/empty/reject policies. Ordered tag patches match original indices/languages.
A resolution records each source outcome, output index, reason, action and warnings.
Plans freeze source digest, executable identities, output absence and expected result.
Jobs retain immutable plan/creator, receipt and terminal facts. A retry is a new job.
Replay scopes bind principal/root and expiry; receipt digest excludes correlation.
Native paths, opaque object IDs and credentials never imply each other's authority.
