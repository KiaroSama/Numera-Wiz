# MuxCls migration and current resolver

Numera Wiz uses one Rust engine for the cleanup adapted from standalone MuxCls
and the copy embedded in FFmWiz. The research inventory records the reviewed
revisions, source comparison and licenses. [Source inventory](../research-pack/research_v7/muxcls-sources) · [Standalone MuxCls](https://github.com/KiaroSama/MuxCls/tree/1269bd71712466469d539fbf279432659ccaa6a2)

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
Matroska remux profile. The shared UI and CLI/MCP adapters call these same
handlers. Use `streams clean` for a file, `batches plan` for explicit inputs,
or `batches plan-folder` for a retained folder snapshot. The folder review
records failures and companion inventory. `continue_with_rejected` reviews
the successfully registered files while retaining the original failure outcomes.
The CLI manual covers requests and submission. [CLI manual](CLI.md)
Single-file and explicit batch extraction accept `.mka` for an audio-only
Matroska output. They retain the original codec, packet payloads and timing.
The remux verifier now requires a complete bounded packet read with exact
rational timestamps, packet hashes/flags, codec/extradata and ffprobe-exposed
side data alongside structural/tag/disposition/chapter assertions. Decoded
reframing fallback, unexposed raw side-data identity, all containers, folder/
companion behavior, full publication recovery and all-interface qualification
remain required.

## Qualification boundary

The hosted domain fixtures cover index-zero, original-language precedence,
no-match, independent attachments and metadata-only decisions. Native filesystem
and process regressions exercise new-byte verification, old-output protection,
flooded independent pipes, cancellation/reaping and distinct launch/child errors.
The real-media job verifies copy bytes, remux language/order/title outcomes,
seekable FD-only probing of a tail-indexed MP4 and rejection of a changed
interior PCM packet before publication. The hosted Linux owner journey also
verified two byte-identical files from a retained folder, matching typed CLI
and both official MCP transport reviews. [Hosted owner batch](https://github.com/KiaroSama/Numera-Wiz/actions/runs/44491743131/job/135456899921)

F08 still requires the remaining container, companion-file and output
policies plus the full native, browser, CLI and MCP acceptance coverage.
[Completion ledger](../development/completion.json)
