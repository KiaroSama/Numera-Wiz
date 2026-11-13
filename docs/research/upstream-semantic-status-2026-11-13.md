# Upstream reading and adoption — 2026-11-13

This record supplements the [observed heads](upstream-head-observations-2026-11-13.json). Observation, source reading, dependency selection and hosted qualification are separate facts. Complete version1 remains unfinished. Foreign program sources and compressed copies stay outside the Git tree.

| Source | Reading completed | Current disposition |
|---|---|---|
| FFmWiz / MuxCls | Requested CLI donor heads checked; both unchanged from the retained review | Behavioral references remain the basis for shared Rust CLI/media rules. No Python/PowerShell runtime is imported. |
| Official Rust MCP SDK | Exact3.10 release/metadata/source delta inspected | rmcp3.10.0 and macro pins adopted. CI277 resolves the exact lock and exercises actual65-tool discovery on four native targets. This does not qualify named Claude/Codex/Cursor loading. |
| redb released4.4→4.5 | All65 available patches and88 commit messages inspected; primary registry4.5 checksum/defaults/dependencies checked | Exact4.5 pin/checksum and the held-file constructor update are prepared in the review branch. Hosted resolution, native storage and old/current package compatibility remain pending; source review is distinct from runtime qualification. |
| redb later master | All seven observed file deltas inspected, including xxh3 changes | Several changes belong to unreleased4.6. A4.5 pin does not include those fixes. |
| egui | Official0.37 release notes and all30 patches/2250 textual lines in the observed master delta inspected | Currenteframe0.36.2 remains; released-version API/closure adoption is pending. A master manifest version alone does not prove a change shipped in0.37. |
| Shotcut | Both observed commits/all21 patches inspected | WASAPI-default migration and overlay viewport-offset behavior inform future Rust audio/preview work; no C++/QML source is imported. |
| FFmpeg | All165 available patches and98 messages in the retained development delta inspected | Stable n9.0.2 remains the selected input. Stable backport presence and candidate binaries require separate verification. Relevant actions below remain required. |
| Requested ffmpeg-skill | Exact observed187856a/3.8.2 skill instructions read | Used as a workflow/reference; no donor executable scripts are added. |
| FilmCraft / EffectCraft / Rust | Heads observed; compare responses reached the300-file API limit | Full changed-file coverage and semantic adoption are still incomplete. These sources are not marked integrated. |

The FFmpeg development comparison spans `9bee1e603cdb6e83d2e99c3cb9a452c2002139c5` to `4345a7b451052025cd5e9632e2725f345baaabfd`. All165 textual patches and98 commit messages were present. Retained aggregate SHA256 is `7f3376721ac9dc86150dd68970a94fd6c00cee2118ec339a5177c70a56d35360`; no unchanged full-file, stable-to-master or binary inspection is implied.

- HTTP/RTSP credential/CRLF fixes matter to future explicitly authorized network sources. The inspected ordinary route admits held local descriptors with an fd-only protocol whitelist. Network fixes do not authorize new URL/secondary-open behavior.
- MOV/HEIF zero-grid rejection and decoder/encoder hardening need stable-component/backport checks. A filename or prefix classifier does not validate all inner codec structures; current deadlines/resource controls remain necessary.
- New seek preroll distinguishes decode from packet copy. Future typed lowering must probe/version-gate options and discard preroll before presentation; it must not apply a development-only option unconditionally to stable binaries.
- Final audio padding, valid sample counts, interlaced/rational rates, Unicode metadata and color tags need operation-specific decoded/roundtrip oracles. Current generated color/silence verification does not cover every affected codec or general media source.
- Ogg whole-copy header fixes do not prove general chained copying correct: retained Vorbis/Opus decoded-copy references still differ, and cut-header references remain different. Successful child exit alone cannot excuse truncation or wrong samples.
- GPU array/allocation/profile changes remain component concerns. They do not qualify an RTX4070Ti route, strict-GPU fallback semantics, process-tree memory or the full CPU-without-vendor-drivers profile.

The released redb comparison spans tags4.4.0 (`7061f7e60d4573311c0636e161d417b93e9fd2b1`) to4.5.0 (`96ce31fe5cafcf735425a02f1781487a4287e2de`). All65 patches/88 messages were present; aggregate SHA256 is `a6f53a891b23ac47d0160ba6f61a6bbef0d78989ac4141455b0e14970b016e12`. Registry4.5 checksum is `8b317ef681e18f6091efbfab532eaf924a135c492d9d0299d0fbfae07e92a39b`; this is primary metadata, not an independently downloaded crate hash.

Defaultstd/dependency declarations/MSRV1.90 remain compatible with the existing closure. Keep defaultExclusiveWriter and the application's separate generation/root guards. The deprecated create_file caller must retain its already-opened create_new File through FileBackend/create_with_backend. Do not enable shared modes/API5 or writable integrity-on-open as a verification probe. Read-only original hashes, raw accepted rows/receipts and the immutable old/current package journeys must remain independent oracles. Explicit-close error propagation is a separate lifecycle/fault decision; later master close/I/O and xxh3 fixes are not established as part of4.5.

Remaining adoption and full-v1 work includes current-source native/browser/CLI/MCP parity, registered/mixed/general media, continuous decoded preview/audio-clock/seek, the complete editor/utility/effect domains, resource/fault qualification, the standard offline four-target component/tool/font/TLS/license/SBOM closure, actual named clients and the concrete final owner-hardware bundle. A read report or green development CI alone does not complete those obligations.
