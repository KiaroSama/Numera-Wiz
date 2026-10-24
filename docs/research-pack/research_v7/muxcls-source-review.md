# MuxCls source review for the Numera Wiz Rust rewrite

**Decision:** MuxCls is an explicit required donor for **F08 / `streams.clean`**, covering the standalone utility and FFmWiz's embedded derivative. The existing F08 already names most of the workflow, but did not identify MuxCls or account for its complete executable source. This review closes that source and behavior identification gap. **It does not claim that a Rust implementation exists.** Implementation, execution and release qualification remain **NOT_IMPLEMENTED / NOT_EXECUTED / NOT_QUALIFIED**.

## 1. Reproducible primary-source scope

| Source | Frozen revision | Evidence and scope |
|---|---|---|
| Standalone [KiaroSama/MuxCls][MC] | `0cf9596f7510f894babf36595efbc827fbbfaf04` | Public repository; default branch `main`; branch/ref and commit retrieved through GitHub read APIs on 2026-10-22. Commit timestamp: 2026-10-04T19:13:33Z. All 58 blobs from its nontruncated recursive tree are archived, with exact Git blob SHA and byte-size agreement. |
| Embedded [KiaroSama/FFmWiz][FW] | `661a0d240ee29ca7cda496b635ae8c50e43d916b` | Existing frozen FFmWiz source inventory. All 17 `ffmwiz/muxcleanup` modules plus the main menu, integration adapter and license are copied into this source archive; their SHA-256 values match the prior inventory. |
| FFmpeg CLI manual | `946fcce07b6dcd0331c8cc609192aeff5e1924f8` | Already archived project manual; streamcopy, mapping, dispositions, metadata and chapters were read for this review. [Streamcopy][FF-COPY], [mapping][FF-MAP], [dispositions][FF-DISP], [metadata/chapters][FF-META]. |

`muxcls-sources/manifest.json` records acquisition and reading status per file. `muxcls-sources/standalone-vs-embedded.patch` records the exact static comparison. A downloaded file is not automatically marked read. All production Python package modules, the standalone entry point, both PowerShell scripts and executable/test configuration were read completely. Test reading coverage is separately recorded. No donor application, Python module import, test, FFmpeg command, build, browser or GPU workload was executed.

The revision, rather than the displayed version string, is the authority: both copies have `APP_VERSION = "1.3.1"` in `__init__.py` and `APP_VERSION = "1.8.0"` in the active constants module. This is source-observed version drift, not evidence of two different reviewed revisions. [Package identity][M-INIT] · [Runtime constants][M-CONST]

## 2. Complete executable-module responsibility map

Every row below was read in full in both package copies. Rust destinations are the mandate's existing logical boundaries, not a proposal for a second engine or 17 new crates.

| Module | Actual responsibility | Required Rust destination |
|---|---|---|
| [__init__.py][M-INIT] | Package marker and stale identity constant. | One Numera application/version identity; donor provenance stays in notices. |
| [constants.py][M-CONST] | Input extension hints, tool names, mode codes, language aliases, timeout/copy/progress defaults. | `nw-domain` typed policy; `nw-engine` runtime profile; no donor numeric mode codes in the public schema. |
| [models.py][M-MODELS] | Probe observations, selection rules, metadata edits and order lists; tolerant number/duration parsing and stream-size estimates. | Shared `nw-domain` asset/stream/request/result types; bounded complete probe records and explicit confidence. |
| [media.py][M-MEDIA] | Tool lookup/version, recursive discovery, ffprobe parsing, scan failures, process control, timeouts and progress parsing. | Existing `nw-platform` admission/process adapters and `nw-engine` probe/supervisor. |
| [prompts.py][M-PROMPTS] | Paths, first positional input, back/quit/defaults, yes/no, language/index entry and output validation. | Thin human CLI forms and shared validation. Machine requests never drive prompts. |
| [selection.py][M-SELECT] | Advanced/exact workflow, audio/subtitle selection, preservation defaults, back navigation and previews. | One typed selector/resolver used by all frontends. |
| [metadata_edits.py][M-EDITS] | Language/title patch entry, edit clearing and independent audio/subtitle partial orders. | Shared F08/F13/F15 tag-patch and order infrastructure. |
| [muxlogic.py][M-LOGIC] | Per-file selection, order, edit precedence, no-op/remux decision and FFmpeg argv. | Pure resolved stream plan in `nw-domain`/`nw-engine`; existing FFmpeg lowering. |
| [output.py][M-OUTPUT] | Rule suffixes, sanitization, unique destinations, relative tree paths, staging names, exclusions and size accounting. | Shared destination claims, native-path policy and artifact publication. |
| [copying.py][M-COPYING] | Chunked copy, timestamps, Windows robocopy branch, unchanged-video copy and optional companion files. | Bounded Rust filesystem stages inside the shared job runtime on every desktop OS. |
| [processing.py][M-PROCESS] | Sequential batch orchestration, per-file outcomes, copy/remux/publish, totals and optional inventory recheck. | `nw-engine` job execution plus `nw-store` artifacts/receipts; actual independent verification. |
| [logsetup.py][M-LOG] | Session logs, environment/tool identity, debug switch, captured-output logging. | Shared structured audit log and command inspector. |
| [reporting.py][M-REPORT] | Scan inventory, aggregate language/title/index choices, preview and settings/result rendering. | Shared typed read models; CLI and `nw-ui` render the same resolution. |
| [progressview.py][M-PROGRESS] | Terminal rows, viewport, ETA, byte/time progress and screen ownership. | Shared job events with bounded CLI/native/browser views. |
| [textutil.py][M-TEXT] | Language normalization, CSV parsing, size/time/index labels and terminal timer. | Shared normalization/formatting; machine validation is strict. |
| [colors.py][M-COLORS] | ANSI palette/stripping and Windows VT initialization. | Human terminal presentation only; FilmCraft-derived GUI remains the selected visual base. |
| [app.py][M-APP] | Discover/probe/select/confirm/process/optional-verify loop and exit handling. | Workflow adapter over `streams.clean`, plan inspection and `jobs.submit`, not a separate core. |

The standalone [MuxCls.py][M-LAUNCH] is a thin import launcher. [run.ps1][M-RUN] sets console encoding/cwd and locates Python; [Install-MuxClsCommand.ps1][M-INSTALL] writes a `MuxCls` function into PowerShell profiles. These are executable packaging behaviors, not features requiring Python, PowerShell or automatic profile modification in Numera. Portable Numera startup must work directly through its own executable. Optional human shell setup remains explicit.

FFmWiz actually exposes menu item **8, Stream Cleanup Remux**, then calls `run_mux_cleanup_mode`. Its adapter imports the subsystem, substitutes `sys.argv = ["MuxCls"]`, permits Back to the parent menu, and obtains the returned processing summary. `base_answers` is not used to populate cleanup rules. Neither this integration nor standalone's first positional input is an unattended typed CLI/MCP interface. [Menu][FW-MENU] · [Adapter][FW-ADAPTER]

## 3. Standalone and embedded are related, but not interchangeable

Twelve of the 17 package modules differ only by the embedded first-line subsystem banner. Five have additional changes: `__init__.py`, `app.py`, `colors.py`, `logsetup.py` and `media.py`. The first four change package description, embedding/back/summary behavior, palette and log-root/timezone compatibility respectively. The source patch records these without interpreting cosmetic differences as new processing features.

`media.py` contains two material embedded improvements absent from the current standalone pin. Standalone returns synthetic exit code **1** on launch/capture failure; the robocopy adapter treats **0–7** as success. Embedded changes these synthetic failures to **127**, outside that tool-specific success range. Rust must represent launch/capture failure as a typed runner error, separate from a successfully started tool's exit status. Embedded also sets explicit UTF-8 replacement decoding for version output, whereas standalone uses the host text default. The embedded comment reports a Windows code-page reproduction; this review confirms the code change, not that reported experiment. [Standalone runner][M-RUNNER] · [Embedded fix][FW-RUNNER]

Do not replace the embedded logic wholesale with standalone on the assumption that the current standalone pin contains every safety fix.

## 4. Behavior that must survive the rewrite

### Selection, metadata and ordering

Audio and subtitles independently support all, none, absolute source stream indices, language membership and case-insensitive title substrings. Multiple title terms mean any match. Resolve the rule separately for every file; aggregate UI choices do not establish that index 2 identifies the same track in every file. Partial order lists put listed retained indices first, deduplicate repeats and append the remaining streams in original relative order. An order entry cannot resurrect a dropped stream. [Resolver][M-LOGIC] · [Selection tests][T-SELECT] · [Order/disposition tests][T-LOGIC]

Actual language normalization trims/lowercases and maps absent/`und`/`unk`/`unknown`/`undefined`/`uknown`/`*uknown` to `und`. It does **not** generally match `en` with `eng`, or `ja` with `jpn`; `language_label` performs friendly aliases for filenames only. Keep matching and display normalization distinct, and use the same explicit normalization policy in F08/F13/F15. [Normalization][M-TEXT]

The advanced wizard defaults to audio-by-language, all subtitles, preserved attachments/global metadata/chapters, companion copying for folder mode, and no overwrite. Audio language values still require input; no particular language is implied. Exact mode asks for explicit indices/all/none. A scan with no audio selects none automatically; a single audio track is still removable. The style-choice menu is skipped when the aggregate audio language set has at most one member, but the advanced workflow still offers exact indices. [Defaults and branching][M-SELECT]

Metadata edits target retained audio/subtitle streams by original language or absolute index. Matching uses original metadata, not earlier rewritten language values. Within the donor model index and language selectors are ORed, and successive applicable nonempty values win per field. Titles/languages cannot be cleared by empty text. Audio/subtitle orders are independent, and the user can clear all pending edits/orders. Numera must express keep/set/clear as typed operations instead of inheriting the empty-string ambiguity. [Edit UI][M-EDITS] · [Edit resolution][M-LOGIC]

### Per-file processing and output

Probe failures remain visible in totals. A recognized filename without a video stream fails validation. When the audio mode is anything other than explicit none and no audio matches, the donor returns `NO_AUDIO_MATCH` and produces no output for that file; this includes a video-only file in a mixed batch under audio-all. Missing subtitle matches do not cause the corresponding skip. Preserve these compatibility decisions explicitly, or label a chosen policy change; do not silently manufacture silent outputs. [Processing decisions][M-DECISIONS]

No change means a complete filesystem copy of the source file. Stream selection/order, effective tag edits, removal of attachments, or disabling global metadata/chapters causes FFmpeg remux. The argv uses explicit video/audio/subtitle/attachment mappings and `-c copy`; it never encodes a replacement codec. The output extension remains the source extension. Explicit per-output audio/subtitle default updates prevent intentional promotion of the first retained track. [No-op and builder][M-BUILD]

The FFmpeg manual distinguishes packet streamcopy from whole-file byte identity, and explains that streamcopy cannot apply frame filters and can still be incompatible with a target container. Mapping order determines output stream order. Default dispositions require deliberate handling, and `-map_metadata -1` without a metadata specifier disables the global mapping, not every per-stream tag. [FFmpeg streamcopy][FF-COPY] · [Mapping][FF-MAP] · [Dispositions][FF-DISP] · [Metadata scopes][FF-META]

Folder mode creates a rule-suffixed output root and preserves relative paths; single-file mode adds a suffix to the filename. Default output base is the input's parent. With overwrite off, the allocator chooses numbered available names; overwrite reuses the requested destination. Folder output equal to or under input is rejected. Optional companion copying includes non-video-extension files, preserves relative paths and reports copied/skipped/failed separately. Source copy carries timestamps where possible without deliberately cloning read-only permission bits. Size savings are the sum for actual successful video outputs, not a subtraction of two directory totals that would count failures as savings. [Output][M-OUTPUT] · [Copy][M-COPYING] · [Summary][M-SUMMARY]

Donor defaults are a 120-second probe timeout, three-hour copy/remux operation timeout, five-second termination grace and 4 MiB copy chunks. Nonpositive `MUXCLS_OPERATION_TIMEOUT` disables its operation limit. These are compatibility facts; Numera uses its shared typed policy and budgets, with finite values and explicit disabled limits, rather than ambient donor environment variables. [Constants][M-CONST] · [Timeout parser][M-MEDIA]

## 5. Confirmed defects and design gaps not to inherit

| Finding | Source-backed consequence | Required decision |
|---|---|---|
| **False success from an old final file** | Zero runner exit plus no new partial can pass if an old final output exists. This is confirmed control flow, not a claim that normal FFmpeg commonly does this. [processing:305–325][M-PUBLISH] | Success must name this attempt's verified owned artifact and publication receipt. Inject this condition in CI. |
| **Verification is optional inventory display** | The prompt defaults no and runs after publication. `verify_output` scans the whole output root, prints counts/languages, returns `None`, and does not compare expected streams/order/tags/content or amend job success. [app][M-APP] · [verifier][M-VERIFY] | Mandatory plan-based verification before publication; optional extra inventory remains a separate read view. |
| **Preservation labels exceed actual semantics** | Remux never maps data streams; byte copy preserves them. Removing subtitles or having none forces all attachments off, despite font wording. Global metadata removal is narrower than clearing all stream tags. Probe records only the default disposition, not all flags. [builder][M-BUILD] · [selection][M-SELECT] · [probe][M-PROBE] | Complete per-stream outcomes; independent data, cover, attachment, tag and chapter policies; full disposition handling. No accidental loss inherited for compatibility. |
| **Staging/overwrite/recovery gaps** | Predictable partial paths are deleted/reused; `exists()` followed by replacement is not atomic no-clobber. The stdlib copy helper always replaces. No durable restart journal appears in this subsystem. Recursive partial exclusion checks basenames only and single-file admission bypasses it. [output][M-OUTPUT] · [copy][M-COPYING] · [discovery][M-MEDIA] | Reuse Numera's unique owned staging, destination claims, source identity checks, publication journal and recovery; never delete by marker alone. |
| **Unconfirmed child termination and finite bounds** | Terminate/kill acts on a direct child; some failures return without confirmed exit, followed by an unbounded wait. Timeout parsing admits NaN/infinity. A synchronous filesystem operation can outlast a between-chunk timeout check. [runner][M-RUNNER] | Shared process-tree ownership and confirmed-exit states; finite validated budgets; document actual filesystem cancellation limits. |
| **Memory and audit gaps** | Both long-run capture files are read wholly into RAM at completion; probe capture is unbounded here. The full batch and progress rows remain in memory. Final argv is DEBUG-only, not always displayed. [runner][M-RUNNER] · [log][M-LOG] · [progress][M-PROGRESS] | Bounded streaming capture/probe/result pages, common budgets, and durable visible final argv before execution. |
| **Duplicate/permissive interface logic** | Preview repeats selector logic separately from execution. Invalid index tokens are dropped, unknown indices can be accepted, and bare `0` means Back even where source index 0 is valid. Standalone does not expose a complete noninteractive result/exit contract. [reporting][M-REPORT] · [prompts][M-PROMPTS] · [prompt tests][T-PROMPTS] | One resolver and typed validation/errors for every interface; human navigation cannot consume a numeric stream identity. |

README states that audio selection is skipped for one or zero tracks, but current source/tests allow removal of a single track. The README also describes installer PATH cleanup that is not present in the reviewed profile-writing script. `tests/test_demo_ui.py` refers to an optional demo directory absent from the frozen tree, with tests skipped when absent. Such text is not evidence of shipped behavior. [README][M-README] · [Selection source][M-SELECT] · [Installer][M-INSTALL] · [Demo test][T-DEMO]

## 6. Test evidence and limits

The frozen repository contains 17 `test_*.py` modules plus fixtures. They cover selection/order/disposition logic, prompt/app flow, output paths, copy behavior, processing rules, process control, progress, logging, runtime imports, a profile installer and FFmpeg-assisted E2E paths. Relevant bodies were read as recorded in the manifest; none were run. The CI configuration declares Windows and Ubuntu Python jobs, not a macOS job. A test file or green-looking test name is not qualification of Numera, macOS or all container combinations. [CI workflow][M-CI]

The E2E source independently probes output languages, audio order and default flags, which is useful fixture intent. Its call to the inventory verifier does not strengthen that verifier. A process-control test whose name says process tree and ignored terminate actually starts one ordinary child without descendants or a signal-ignore handler; escalation is separately exercised with a stand-in object. Do not inherit its title as proof of descendant cleanup. [E2E tests][T-E2E] · [Process-control tests][T-PROCESS]

The companion contract and coverage CSV connect these observations to concrete Rust obligations and the existing F08 acceptance suite. Missing runtime evidence remains a required CI gate. GPU testing stays on the owner's machine as already directed, but streamcopy/byte-copy itself has no GPU decode/filter/encode stage to qualify.

## 7. Licensing and completion language

Standalone MuxCls carries an MIT license and attribution to KiaroSama; embedded FFmWiz carries the previously archived GPL license. Retain the applicable copyright/license notices and source provenance when translating or adapting substantial source, within Numera's already selected GPL-3.0-or-later release process. Do not relabel the donor's original license as GPL or discard its notice. [Standalone license][M-LICENSE] · [Attribution][M-ATTR] · [Embedded license][FW-LICENSE]

The accurate current claim is: **“MuxCls's standalone and embedded cleanup behavior is explicitly required and source-mapped for a complete first-party Rust rewrite under F08; implementation and qualification are still pending.”** “Fully rewritten” becomes true only when the corresponding Rust handlers, all four interfaces, packaged targets and required CI evidence exist.

[MC]: https://github.com/KiaroSama/MuxCls/tree/0cf9596f7510f894babf36595efbc827fbbfaf04
[FW]: https://github.com/KiaroSama/FFmWiz/tree/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup
[M-INIT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/__init__.py
[M-CONST]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/constants.py
[M-MODELS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/models.py
[M-MEDIA]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/media.py
[M-PROMPTS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/prompts.py
[M-SELECT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/selection.py
[M-EDITS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/metadata_edits.py
[M-LOGIC]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/muxlogic.py
[M-OUTPUT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/output.py
[M-COPYING]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/copying.py
[M-PROCESS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/processing.py
[M-LOG]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/logsetup.py
[M-REPORT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/reporting.py
[M-PROGRESS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/progressview.py
[M-TEXT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/textutil.py
[M-COLORS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/colors.py
[M-APP]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/app.py
[M-LAUNCH]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/MuxCls.py
[M-RUN]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/run.ps1
[M-INSTALL]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/Install-MuxClsCommand.ps1
[FW-MENU]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/FFmWiz.py#L362-L376
[FW-ADAPTER]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext00b.py#L172-L230
[M-RUNNER]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/media.py#L177-L269
[FW-RUNNER]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup/media.py#L178-L295
[M-DECISIONS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/processing.py#L163-L267
[M-BUILD]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/muxlogic.py#L173-L263
[M-SUMMARY]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/processing.py#L338-L413
[M-PUBLISH]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/processing.py#L305-L325
[M-VERIFY]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/processing.py#L424-L456
[M-PROBE]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/muxcls/media.py#L315-L381
[M-README]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/README.md
[M-CI]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/.github/workflows/tests.yml
[T-SELECT]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/tests/test_selection_logic.py
[T-LOGIC]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/tests/test_muxlogic.py
[T-PROMPTS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/tests/test_prompts.py
[T-DEMO]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/tests/test_demo_ui.py
[T-E2E]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/tests/test_processing_e2e.py
[T-PROCESS]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/tests/test_process_control.py#L42-L82
[M-LICENSE]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/LICENSE
[M-ATTR]: https://github.com/KiaroSama/MuxCls/blob/0cf9596f7510f894babf36595efbc827fbbfaf04/ATTRIBUTION.md
[FW-LICENSE]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/LICENSE
[FF-COPY]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L265-L325
[FF-MAP]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L2190-L2240
[FF-DISP]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L923-L940
[FF-META]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L2307-L2352
