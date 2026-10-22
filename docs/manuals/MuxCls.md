# MuxCls migration and current resolver

The complete standalone/embedded MuxCls rewrite is required initial scope under
F08. It is not complete yet. The original source, MIT notice, comparison and
behavior ledger remain in `docs/research-pack/research_v7/muxcls-sources/`.

## Current implemented internals

The common Rust resolver accepts explicit all/none/source-index/language/title
selectors, no-match policy, separate partial audio/subtitle orders, ordered
keep/set/clear tag patches and independent preservation categories. It retains
valid source index 0. Exact language matching trims/lowercases and maps unknown
values to `und`; `en` and `eng` remain different. Title matching is substring OR,
without regex. Original-source predicates do not begin matching a language that
an earlier patch merely wrote. The last explicit patch wins per field.

Partial ordering deduplicates named retained streams and appends unlisted retained
streams in their original order. Missing/dropped indices are reported, never
reintroduced. Subtitles and attachments are independent. Every source gets an
output index or a precise dropped/skipped outcome. A keep-everything resolution
is byte-copy; metadata/order/chapter changes are remux decisions. An explicit
no-audio-match skip has no produced artifact.

The engine internals implement bounded regular-file probing, source/component
hashes, immutable no-replace plans, verified Rust byte copying and a conservative
Matroska remux profile. They are not exposed as a completed media/job interface.
The remux verifier currently makes structural/tag/disposition/chapter assertions;
packet/decoded/timing preservation, all containers, folder/companion behavior,
full durable owner/publication recovery and all five interfaces remain required.

## Qualification boundary

The hosted domain fixtures cover index-zero, original-language precedence,
no-match, independent attachments and metadata-only decisions. Native filesystem
and process regressions exercise new-byte verification, old-output protection,
flooded independent pipes, cancellation/reaping and distinct launch/child errors.
The real-media job builds the pinned FFmpeg source into a limited CPU fixture
profile and checks copy bytes plus actual remux language/order/title outcomes.
This fixture profile is not the complete standard distribution profile.

Only after every Annex D coverage/acceptance row has actual supported-platform,
package and five-surface evidence may the project claim a full MuxCls Rust rewrite.
