> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## Annex D. Complete MuxCls Rust rewrite

### D.1 Binding scope and truthful status

**Implement the complete useful cleanup workflow of standalone MuxCls and its embedded FFmWiz derivative as first-party Rust inside Numera Wiz.** This is the explicit named source baseline for **F08 / `streams.clean`**. Do not ship a Python wrapper, invoke MuxCls as a subprocess, require robocopy, or build a second cleanup engine. Preserve the donor's useful behavior, replace the confirmed defects below, and reuse Numera's existing probe, selectors, metadata, plan, job, artifact, verification and transport infrastructure.

Frozen sources are standalone `KiaroSama/MuxCls@0cf9596f7510f894babf36595efbc827fbbfaf04` and embedded `KiaroSama/FFmWiz@661a0d240ee29ca7cda496b635ae8c50e43d916b`, under `ffmwiz/muxcleanup`. All 17 package modules in each copy, the standalone launcher/setup scripts, relevant configuration and all 17 standalone test modules were read as source. Primary files, exact integrity hashes, the cross-copy diff and per-file reading status are in `research_v7/muxcls-sources/`. The companion source review explains the evidence and limitations. [Standalone source][MC] · [Embedded source][FW]

**Current status: NOT_IMPLEMENTED / NOT_EXECUTED / NOT_QUALIFIED.** A complete source inventory or written contract does not mean a complete Rust rewrite. Do not change these statuses until the corresponding implementation and recorded CI/package evidence exist. Source line counts are not a completion criterion.

This annex refines F08 and the existing shared runtime contracts. It does not replace the FilmCraft-derived GUI, add an alternative media engine, or weaken mandatory verification, resource budgets, source grants, process ownership, portable packaging or cross-interface parity. Native GUI and the visually matching browser GUI use the same cleanup workspace and local engine. CLI, stdio MCP through `bridge`, and direct HTTP MCP through the existing configurable loopback listener at default port **1578** expose the same operation and results.

### D.2 Every executable module has an implementation destination

These are responsibilities within the already selected workspace. They are not 17 new crates, and rewriting a module does not require preserving its Python file boundaries.

| Donor module | Required behavior/responsibility | Existing Rust owner |
|---|---|---|
| `__init__.py` | Package identity/provenance; eliminate conflicting donor version constants from runtime identity. | App identity and notices. |
| `constants.py` | Typed selection/preservation/timeout defaults, input-family hints, unknown-language policy and bounded copy/progress settings. | `nw-domain`, `nw-engine`. |
| `models.py` | Assets, complete stream observations, rules, tag patches, partial orders, estimated sizes and unknown values. | Shared `nw-domain` models. |
| `media.py` | Admitted discovery, bundled-tool identity, bounded probing, scan failures, process lifecycle and telemetry. | `nw-platform`, `nw-engine`. |
| `prompts.py` | Human path/selection forms, navigation and validation; no prompts in machine mode. | CLI adapter, `nw-ui`, shared validators. |
| `selection.py` | All/none/language/title/index selectors, mode defaults, previews, per-file matching and explicit no-match policy. | Shared selector resolver and command descriptor. |
| `metadata_edits.py` | Audio/subtitle language/title edits, clear-edit action, independent partial orders. | Existing F08/F13/F15 tag/order service. |
| `muxlogic.py` | Resolve selected streams, metadata precedence, copy/remux decisions and exact FFmpeg maps/options. | `nw-domain` plan data and `nw-engine` lowering. |
| `output.py` | Output root/tree/suffix naming, collisions, identity checks, staging, exclusions and size accounting. | Existing native-path allocator, publication claims and artifact journal. |
| `copying.py` | Bounded unchanged-file and companion-file copying, requested timestamps, overwrite and per-file outcomes. | Rust filesystem stage in the common job runtime. |
| `processing.py` | Batch dispatch, every-file result, no-match/skip/failure, verification, publication and totals. | `nw-engine`, `nw-store`, `nw-service`. |
| `logsetup.py` | Tool/profile identity, session/job diagnostics, command records and support data. | Shared structured audit log/command inspector. |
| `reporting.py` | Probe inventory, aggregate choices, resolved preview, per-file outcomes and summaries. | Shared typed queries/results, then CLI/GUI rendering. |
| `progressview.py` | Progress/ETA/unknown states, finished count, bounded viewport and redirected-output behavior. | Common job events and UI/CLI adapters. |
| `textutil.py` | Language matching normalization, independent display labels, finite units and safe presentation. | Shared domain utilities and presentation formatters. |
| `colors.py` | Human terminal coloring/cursor capability; no protocol contamination. | `nw-platform` CLI renderer; no GUI palette duplication. |
| `app.py` | Input→scan→selection→review→plan→job→result workflow and explicit failure/cancellation semantics. | Thin workflow over canonical commands. |

`MuxCls.py`, `run.ps1` and `Install-MuxClsCommand.ps1` become unnecessary runtime dependencies. The portable Numera executable launches the workflow directly. An optional documented shell alias must be explicitly installed by the human; ordinary startup must not edit shell profiles, PATH or execution policy. Embedded FFmWiz's `run_mux_cleanup_mode` becomes an ordinary route to the same utility workspace. The donor's `base_answers` parameter is not evidence of an existing machine configuration interface. [Integration adapter][FW-ADAPTER] · [Standalone launch/setup files][MC]

### D.3 Canonical typed interface and plan

Keep the existing request identity:

```text
streams.clean {
  assets, audio_selection, subtitle_selection,
  audio_order, subtitle_order, stream_tag_patches,
  preservation, output_tree, copy_non_media, overwrite, verify
}
```

Extend the shared typed field descriptors where needed; do not introduce another cleanup request engine or unvalidated string command. The standard command envelope, schema version, principal, grants and durable receipts remain authoritative. The processing intent returns an immutable execution plan. `jobs.submit` accepts that plan; `jobs.cancel`, inspection, events and retries retain their existing semantics. A utility job works without creating a timeline/project.

| Existing request field | Binding interpretation |
|---|---|
| `assets` | Authorized registered file IDs or an authorized finite folder-discovery snapshot. Native paths are resolved by the existing admission service; browser-local path strings are not engine file access. Every discovered candidate has an outcome. |
| `audio_selection`, `subtitle_selection` | Tagged selector: all, none, source indices, normalized language membership, or case-insensitive title substring-any. Each carries its explicit permitted no-match behavior. Required selector values cannot be supplied by hidden interactive defaults. |
| `audio_order`, `subtitle_order` | Optional source-index partial orders resolved independently per file against the retained set. |
| `stream_tag_patches` | Ordered typed keep/set/clear edits over retained streams, using the shared F13/F15 matcher and explicit source identity. Source-language matching uses the original probe snapshot. |
| `preservation` | Separate ordinary/additional video, attached-picture, data/unknown-stream, attachment, global-tag, per-stream-tag, chapter and disposition policies. A blanket keep-metadata switch cannot stand for all of them. |
| `output_tree` | Single-file suffix or relative-tree layout, authorized output root, explicit naming/collision policy and original container by default. Preserve the source/output identity protections. |
| `copy_non_media` | Explicit companion-file policy for a folder input; its bounded file manifest and results are part of the same job. Single-file mode has no implicit sibling traversal. |
| `overwrite` | Existing explicit no-replace/replace policy, tied to the reviewed target identity or absence. It does not permit arbitrary later replacement of a changed target. |
| `verify` | The existing `standard`, `full_decode` or `preservation` profile, with F08-specific obligations. Mandatory identity/structure/assertion checks cannot be disabled by a donor-style boolean. |

The resolved plan must show, for every candidate, the source identity, selected source indices, output index/type order, retained/dropped stream reasons, final tag/disposition/chapter policy, requested destination, and decision: copy unchanged, remux, explicit skip, or rejected input. Resolve previews once with the same code execution consumes. Do not reimplement matching in the GUI, report generator, CLI or MCP adapter.

Every valid source-index value, including **0**, is available to every machine and human surface. The human UI uses separate Back navigation. Reject malformed/negative/out-of-range numeric encodings and nonfinite timeout/size/duration values with field-level errors; never silently discard an invalid CSV token and execute the remainder. A legacy index absent from every candidate requires an explicit recorded unresolved-index policy; missing matches within individual files remain visible per-file outcomes.

### D.4 Defaults and selector semantics

The table distinguishes source behavior from the selected Rust rule. A behavior change is a reviewed safety or clarity improvement, not an accidental compatibility difference. [Selection][M-SELECT] · [Resolver][M-LOGIC] · [Normalization][M-TEXT]

| Concern | Donor behavior | Numera rule |
|---|---|---|
| Advanced entry | Audio-by-language with values required; subtitles all. | Preserve the human starting choices. Serialize the final explicit selector; machine requests never guess a language. |
| Exact entry | Prompt for exact indices or all/none. | Same capability through the shared tagged selector; no separate execution path. |
| No audio in scanned batch | Select audio-none automatically. One audio track remains selectable/removable. | Preserve the visible resolution; a single stream is never hidden from selection. |
| One distinct audio language | Skip style-choice menu and enter advanced mode, which still offers indices. | Presentation convenience only. Every selector remains discoverable in all interfaces. |
| No matching audio | Skip the file unless audio-none was explicit. Video-only input under audio-all is also skipped. | Default compatibility policy is `skip_file` with `NO_AUDIO_MATCH`; the preview enumerates it. A distinct explicitly requested silent-output policy may proceed, but must never be inferred. |
| No matching subtitles | Process with no selected subtitles. | Preserve as declared empty-subtitle policy and visible warning/result. Do not automatically remove unrelated attachments. |
| Metadata/chapters | Keep by default. Donor metadata toggle affects the global mapping. | Keep the applicable independent metadata/chapter policies by default. Removal targets explicit scopes and is verified. |
| Attachments | Keep initially, but forcibly off if subtitles are absent/removed. | **Fix:** independent retain-by-default attachment policy; removal is explicit. Font and other attachment types are not interchangeable. |
| Companion files | On by default for a folder; off for a single input file. | Preserve the folder-workflow default only as an explicit serialized policy, showing the candidate manifest/limits. Never implicitly scan siblings of a single input. |
| Overwrite | Off; choose numbered output names. On reuses the requested output. | Preserve these naming choices through atomic allocation/publication. A newly conflicting target fails or is replanned; it is not silently renamed after review. |
| Timeouts | Probe 120 s; operation 10,800 s; termination grace 5 s. Nonpositive operation env value disables its bound. | Expose these as the initial cleanup compatibility budget through the shared policy. Explicit disabled is a typed state subject to service policy; reject NaN/infinity. No ambient `MUXCLS_*` control of accepted jobs. |
| Copy buffer/progress | 4 MiB chunks; 0.2 s polling in donor. | Use a bounded Rust copy buffer, initially 4 MiB per active copy; shared scheduler/event throttling applies. Do not add polling subprocesses or per-file background threads. |

Language matching trims/lowercases and maps the donor's unknown tokens to `und`. It must preserve an explicit exact-normalized-tag compatibility mode: `en` and `eng` are not automatically equal merely because the filename display says EN. If shared canonical language aliasing is also offered, it is a named normalization choice resolved in the plan and identical across F08/F13/F15. The display helper must not silently change selection.

Title selection uses the documented case normalization, no regex evaluation, and OR across requested substrings. For a partial order, retain the first occurrence of each listed retained stream, then append unlisted streams in original relative order. An index naming a dropped/missing stream has no order effect and is reported; it never reintroduces the stream. Freeze source identity and revalidate before execution so changed files cannot turn index 2 into an unreviewed selection.

Metadata patch predicates resolve against the original source snapshot. For the donor-compatible composite predicate, original language and source-index matches are ORed within the declared stream kind. Ordered applicable patches resolve each field deterministically, with the last explicit set/clear winning. Unset means keep. Empty string is not an ambiguous synonym for both unchanged and clear. Apply output-indexed options after resolved ordering. Clearing the pending-edit list is a UI/request-edit action; it does not mutate source metadata.

### D.5 Media decision and strict-copy lowering

First admit/probe the source through the existing closed-reference and bounded-probe policy. The donor's six extensions (`mkv`, `mp4`, `m4v`, `webm`, `mov`, `avi`) establish initial discovery hints, not permission to invoke arbitrary demuxers or proof that a renamed file is safe. A cleanup candidate without an admitted video stream fails this video-utility workflow; audio-only operations continue through their existing commands. Failed probes and unsupported sources stay visible; continuing with the supported subset is an explicit batch policy, not silent disappearance.

Classify each supported file from its **complete resolved intent**:

1. **Copy unchanged.** Every retained stream, order, container, tag, chapter, disposition and other relevant preservation policy is already satisfied. Produce a new output through the Rust file-copy artifact stage. This is real work with progress/verification/publication, not an empty success. Byte-copy preserves all source bytes, including structures FFmpeg would not remux. A filesystem timestamp/naming choice does not itself require media remux.
2. **Remux.** Selection/order/tag/chapter/attachment or other admitted container changes require a new container. Use explicit source-index maps and `-c copy`; no decode/filter/encode stage is added. Metadata-only and order-only changes are not ignored. A policy requesting strip/normalization may require remux even when a bounded probe cannot establish that every removable structure is absent.
3. **Skip/reject.** Preserve the declared no-audio-match, explicitly allowed existing-file skip, unsupported-input or incompatible-stream outcome. No output artifact may be claimed for a skipped item.

The remux planner must enumerate all streams rather than relying on donor `-map 0:v?` plus a few audio/subtitle maps. Extra ordinary video, attached pictures, subtitle codecs, font/file attachments and data/unknown streams are separate outcomes. Keep source dispositions unless the request explicitly changes them; do not assign a new default merely because the original default was removed. Qualify full disposition preservation for each admitted muxer, not only the `default` flag that the donor probes.

Use separate global/per-stream metadata and chapter controls; `-map_metadata -1` alone is not a universal tag scrub. Strip or retain container/muxer-generated tags according to the documented output profile, and distinguish unavoidable generated signaling from user tags. Preserve source A/V offsets under the existing timestamp policy. Any admitted timestamp or bitstream-header normalization is disclosed and covered by the chosen oracle.

Maintain source container by default. Preflight requested copied streams against the actual bundled muxer/profile. If an attachment, bitmap subtitle, data stream, disposition or required tag cannot survive, return a precise incompatible-preservation result. The user can choose an existing separate conversion/output policy; cleanup must not silently drop it, transcode it, or claim that `-c copy` makes every combination valid. FFmpeg packet-copy is not whole-container byte identity. [FFmpeg streamcopy][FF-COPY] · [Map order][FF-MAP] · [Dispositions][FF-DISP] · [Metadata/chapters][FF-META]

This workflow has no GPU media stage. It runs in CPU-only installations without hardware initialization; Auto need not discover a device to copy/remux. Apply the existing precise `GpuRequired` stage policy rather than inserting GPU decode/encode to satisfy a badge. Broader application GPU qualification remains on the owner's system as requested.

### D.6 Cross-platform copying, publication and recovery

Use one Rust copy policy on Windows, Linux and macOS. It may use qualified native fast-copy primitives, but must preserve the same observable semantics and provide a bounded-buffer fallback. No Python runtime, PowerShell host, robocopy or shell-form command is required. Do not infer a speed advantage from donor `/J` comments; measure the packaged implementation under the existing performance suite.

Folder jobs preserve the requested relative tree and list companion copies separately. Traverse only authorized regular files and admitted references. Reuse the shared symlink/reparse/native-path rules; prune output and owned staging subtrees before descending. A marker in a filename is neither an ownership proof nor a complete exclusion mechanism. Resolve output/source aliases and case/normalization collisions with the qualified filesystem rules, including an absent destination. Do not treat a resolution error as proof that two paths are distinct.

Copied outputs preserve bytes. Requested file timestamps are preserved where supported; filesystem permissions, ownership, ACLs, executable bits and extended attributes follow the explicit existing copy policy, not a blanket claim that every filesystem property is cloned. In compatibility mode, timestamp-copy failure is a recorded warning unless timestamps are an explicit required assertion. Overwrite must actually replace the reviewed old bytes even when its size/mtime equals the source; no-overwrite leaves the existing file untouched and records the declared skip/conflict. Single-file source aliases are never overwritten.

Every output uses a unique attempt-owned staging artifact, common publication journal and destination claim. Staging preserves or explicitly supplies muxer format, so random temporary naming cannot select the wrong container. Validate the newly produced artifact before the publication fence. An exit code of zero, an old final path, a stale partial or equal file sizes is insufficient evidence of success. The confirmed donor branch `returncode == 0 && final.exists()` must never appear as the success oracle. [Confirmed branch][M-PUBLISH]

No-replace publication uses the qualified atomic operation, not an `exists()`/replace race. Replacement requires the frozen target identity and explicit policy. Cleanup deletes only artifacts proven to belong to this job/attempt, after confirming no writer remains. Publication failure, cleanup failure, disk full, denied access and changed sources remain distinct typed outcomes. Do not recursively remove a predictable partial directory just because its name resembles the donor marker.

The batch is a collection of independently journaled artifacts, not a promised atomic directory transaction. Completed verified outputs remain available when later files fail or cancellation arrives. Recovery reacquires required ownership/claims, reconciles each recorded intent, and resumes/retries only under the common source/target/receipt rules. It must not count pre-existing files as this job's output or start a fresh duplicate job from an uncertain response.

### D.7 Verification, result accounting and bounded operations

Use mandatory verification on the owned staging artifact **before publication**. Compare the expected stream outcome manifest: types, codecs, output order/source correspondence, relevant layouts/parameters, dispositions, tags, chapters, attachments/covers/data, time extent and source offsets. Verify every required artifact, not all files that happen to exist under an output directory. A required missing file fails even if an unrelated existing file looks healthy.

The selected preservation oracle follows the main mandate: full byte identity for a true file copy; admitted packet identity where framing is unchanged; or canonical decoded/semantic equivalence where an explicitly admitted container/bitstream conversion prevents packet-byte equality. Structural checks alone must be labeled as such. A full-read checksum adds I/O and is budgeted/reported; sampled checks do not justify a full-byte identity claim. Extra `full_decode` or preservation work remains visible/cancellable and shares scheduler limits. The donor's optional `verify_output` inventory is retained as a useful report view, not as the success gate. [Donor verifier][M-VERIFY]

Results include every candidate and companion file with action, reason, source/output identity, artifact/receipt ID where one exists, typed error/warning, elapsed time, verification depth/outcome and measured byte sizes. Report separate remuxed, copied unchanged, accepted skip, no-audio-match, failed, cancelled and companion outcomes. Aggregate success cannot hide failed required items; CLI exit and MCP/domain error/result mapping follow the shared batch contract. An accepted skip is not a successfully produced artifact.

Size change is the sum of actual successful per-file output-size minus input-size. A failed/skipped input contributes no fictitious saving. Companion bytes are reported separately from video size reduction. Keep elapsed operation time, verification time and end-to-end total so performance comparisons include required work.

Timestamp progress may use validated source duration; unknown duration remains unknown. Byte counters report bytes actually processed/written. Source-size ratios during stream removal are estimates, not exact output-completion percentages. Finished-item progress counts failed/skipped terminal items as finished, but exposes success counts separately. GUI/browser lists are paged/virtualized; CLI redirected/JSON output emits no cursor control. MCP stdout is protocol-only.

Use bounded probe parsing, incremental telemetry, capped diagnostic tails and rotating structured logs. Never read the entire lifetime stderr/stdout capture into RAM at process end. Batch inventories/results may spill to the existing store rather than retaining every record in GUI memory. Shared admission/backpressure and aggregate owned-process budgets still apply.

All exact executable argv vectors and effective policies are inspectable through the common plan/stage command inspector before execution, with appropriate credential redaction; debug mode is not required to see them. Preserve the authoritative argv, not a platform-specific display string executed through a shell. Typed launch/capture failures cannot be confused with a child return-code convention—the embedded 127 fix must become a proper runner error, not a second robocopy compatibility branch. [Standalone runner and embedded correction][FW-RUNNER]

Cancellation uses the common token and owned process groups/Windows Job Objects. Distinguish request-to-stop from confirmed stopped/reaped state. Do not publish/delete while a writer may remain alive. Copy cancellation checks bounded chunks, and qualified operating-system behavior defines the actual limit of blocked filesystem I/O. A nonfinite timeout is invalid; disabled deadlines are explicit. The donor's claim that a child is always reaped and its process-tree test name are not evidence of that guarantee. [Donor control][M-CONTROL] · [Test body][T-PROCESS]

### D.8 Specific source defects and unsupported claims to resolve

The implementation agent must track these as explicit F08 regressions/improvements, with source evidence retained:

- **Old-output false success:** require this attempt's new verified artifact; inject rc=0/no-staging with an old destination in CI.
- **Optional postpublication inventory:** replace with required expected-output assertions before publication; the old inventory can still be shown.
- **Attachment/subtitle coupling and missing data maps:** independent policies and complete outcomes; preserve attached pictures as their own category.
- **Narrow metadata/default-only probe:** global strip does not mean all stream tags; full disposition and metadata requirements come from the shared model.
- **Predictable partial cleanup and no-overwrite races:** shared ownership/claims/journal; no filename-based deletion or unchecked replacement.
- **Launch code collision and version decoding:** typed launch/capture failures and qualified bounded decoding; standalone is not a safety superset of embedded.
- **Unbounded output capture/nonfinite timeout/direct-child assumptions:** bounded parsing, finite typed policy and shared confirmed termination/recovery.
- **Preview duplication/index-zero collision/permissive CSV:** one resolver and strict schemas; human Back is separate from valid stream index zero.

Do not port obsolete README claims as requirements: the current source lets the user remove a single audio track; the reviewed installer does not implement the README's stale PATH-cleanup description; absent optional demo sources are not a shipped GUI. The source has no durable resume implementation to reuse. macOS support in prose is not packaged qualification, and historical copy timing comments are not Numera performance results. These facts and exact primary anchors are detailed in the source review.

### D.9 Focused CI acceptance journeys

These extend F08-A through F08-E and the existing shared-runtime gates. They do not authorize local ordinary tests, builds or FFmpeg execution. Required application builds/tests run in GitHub CI against actual packaged Windows, Linux and macOS targets; GPU-specific qualification remains owner-local and is unnecessary for copy/remux itself. Tests below are required assertions, currently **NOT_IMPLEMENTED / NOT_EXECUTED / NOT_QUALIFIED**.

**MX7-J01 — Complete selection and per-file resolution (WP04/WP07):** On heterogeneous multi-stream fixtures, exercise all/none/language/title/source-index audio and subtitle selectors, including index 0, mixed case, unknown-language aliases, one audio track, no audio and absent per-file indices. Assert the complete selected/dropped source-index ledger, default `NO_AUDIO_MATCH` skip with no artifact, explicitly requested no-audio output, and empty-subtitle behavior. Confirm `en`/`eng` differ in exact-tag mode and that any named canonical-alias mode produces the same resolution on every interface.

**MX7-J02 — Partial order and metadata precedence (WP04/WP07):** Reorder audio/subtitles with repeated, unlisted, dropped and missing indices; assert retained order and output-indexed options. Apply overlapping original-language/index edits, then change a language again, set/clear titles and clear pending edits. Assert original-snapshot matching, deterministic last-explicit-patch precedence and unchanged dropped streams. Metadata-only, chapter-policy-only and order-only changes must produce the declared remux action and independently verified output.

**MX7-J03 — All preservation categories and strict-copy incompatibility (WP04/WP07):** Use admitted fixtures containing extra video, an attached cover, font and non-font attachments, text/bitmap subtitles, data where supported, chapters, global/stream tags and default/forced/commentary/original dispositions. Removing subtitles must not remove independent retained attachments. Assert every stream/tag/disposition outcome, no automatic new default, and preserved offsets. An incompatible retained stream or muxer requirement fails before execution or publication; no hidden drop, transcode or unsupported-preservation claim is accepted.

**MX7-J04 — True no-op copy and unchanged media evidence (WP04/WP07):** A keep-everything request selects the Rust byte-copy stage, creates a new output and invokes no FFmpeg transform/encode stage. Compare complete source/output bytes under the selected byte-identity oracle. A genuine stream/metadata/order change selects strict remux; verify the appropriate packet or canonical preservation oracle, accounting for explicitly admitted header normalization rather than falsely requiring identical container bytes. Report verification cost and scope.

**MX7-J05 — Output trees, native paths and companion semantics (WP04/WP07):** Process a recursive tree with Unicode/Persian, spaces, leading-dash names, case/normalization collisions, long paths within the admitted platform limit, aliases and owned staging subtrees. Assert stable relative layout, exclusion before traversal, source/output identity rejection, explicit numbered-name allocation, and no unauthorized symlink/reparse traversal. In overwrite mode replace different bytes even when size/mtime match; in no-overwrite mode leave existing content untouched. Companion failures and skips remain distinct structured outcomes.

**MX7-J06 — No new artifact cannot be success (WP04/WP07):** Inject a runner that returns zero while creating no new staging file, with an old final output present. Also supply a stale foreign partial, an empty/truncated new artifact and an unrelated valid output in the folder. All fail the owned-artifact/expected-output gate, preserve the old destination, and create no success/publication receipt. Validate this deterministically without depending on FFmpeg to exhibit the fault naturally.

**MX7-J07 — Independent verification catches semantic mismatch (WP04/WP07):** Produce valid containers with deliberately wrong selected language, order, disposition, tag/chapter/attachment policy, offset or expected-file coverage. The verifier must reject the relevant mismatch before publication, even when exit code, file existence and broad video/audio counts look correct. A full-byte claim with equal size but different content must fail. Verify that a separate inventory report cannot change an unverified artifact into success.

**MX7-J08 — Collision, cancellation and recovery boundaries (WP04/WP07):** Race cooperating owners at one destination, cancel during copy/remux/verification, interrupt before and after the publication fence, and restart with changed source/target identities plus foreign partial names. Assert atomic no-replace behavior, reviewed replacement identity, no deletion outside owned artifacts, accurate already-published results and recovery through existing receipts/journal. Completed outputs remain valid; no whole-batch atomicity or automatic duplicate retry is implied.

**MX7-J09 — Process faults and finite operation limits (WP04/WP07):** Inject launch/capture failure separately from child exit codes, malformed/nonfinite timeout values, a real owned descendant tree, ignored graceful termination and a termination failure. Assert typed errors, bounded escalation/reaping evidence and no publication/cleanup while a writer remains active. Qualify cancellation behavior for the admitted copy adapter without claiming a synchronous blocked filesystem call is instantly interruptible.

**MX7-J10 — Bounded resources, progress and truthful totals (WP04/WP07):** Run a large paged batch and verbose diagnostic fixture under the shared memory/log/scratch caps. Assert no whole-output capture spike, no unbounded UI list, nonblocking protocol output and explicit dropped/coalesced diagnostic accounting. Unknown progress remains unknown; byte estimates are labeled; failed/skipped files count as finished but not successful. Only successful video deltas contribute to video savings, with companion bytes and verification time reported separately.

**MX7-J11 — Native/browser/CLI/MCP parity (WP03/WP04/WP07):** Submit the same normalized cleanup intent through the desktop utility, matching browser utility, CLI JSON, stdio `bridge` and direct HTTP MCP at the default and one nondefault port. Under identical immutable source/target snapshots and equivalent principals/grants, compare normalized semantic plan projections: selector/default policies, maps, warnings, command inspection, events and expected outcomes. Exclude incidental generated identities from that comparison; independent submissions may create different plan/job IDs, and a changed output target requires the normal newly reviewed plan. For an exact request retried by the same principal within the same replay scope, assert the original accepted receipt/job identity. Machine modes require no prompt, GUI session, Python or PowerShell. Browser reconnect and MCP request cancellation must not duplicate or implicitly cancel an already accepted job.

**MX7-J12 — Portable CPU-only package and rewrite completion (WP01/WP04/WP07):** From each CI-built desktop package, run the cleanup workflow without Python, PowerShell, robocopy or FFmpeg on PATH, using only the qualified bundled FFmpeg/ffprobe pair and Rust filesystem adapter. CPU-only copy/remux must perform no media GPU initialization. Check all module/behavior rows in `muxcls-coverage.csv` against actual handlers, tests and package evidence, with preserved source-license notices. Missing behavior or a failed platform gate keeps the full-rewrite claim blocked.

### D.10 Completion gate and repository documentation

`muxcls-coverage.csv` is the behavior-to-source-to-Rust-to-CI ledger. Keep module coverage and behavior coverage distinct. Every required row needs an implemented handler/owner and concrete evidence, or an explicit unresolved blocker; a unit test count or converted line count cannot close it.

Document the utility in the human and agent README setup paths and in the separate complete command reference. Include all selector variants, defaults, no-match behavior, partial order, keep/set/clear tags, exact copy versus remux, companion files, overwrite/target identity, timeout/cancellation, verification scope, result/exit semantics and CPU-only operation. Examples must use generated current schemas and executable CLI forms, with a preview then job submission; do not copy an interactive transcript as an agent API.

Preserve MuxCls's original MIT notice/attribution and the applicable embedded FFmWiz notices within the existing GPL-3.0-or-later release process. Mark source translation, retained behavior and intentional defect fixes in provenance. The release can state **“MuxCls functionality fully rewritten in Rust”** only after these gates pass on the supported package matrix; the FFmpeg third-party binaries remain honestly identified as non-Rust media dependencies.

[MC]: https://github.com/KiaroSama/MuxCls/tree/0cf9596f7510f894babf36595efbc827fbbfaf04
[FW]: https://github.com/KiaroSama/FFmWiz/tree/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup
[FW-ADAPTER]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext00b.py#L172-L230
[M-SELECT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/selection.py
[M-LOGIC]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/muxlogic.py
[M-TEXT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/textutil.py
[M-PUBLISH]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/processing.py#L305-L325
[M-VERIFY]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/processing.py#L424-L456
[M-CONTROL]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/media.py#L39-L90
[FW-RUNNER]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup/media.py#L178-L295
[T-PROCESS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/tests/test_process_control.py#L42-L82
[FF-COPY]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L265-L325
[FF-MAP]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L2190-L2240
[FF-DISP]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L923-L940
[FF-META]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L2307-L2352
