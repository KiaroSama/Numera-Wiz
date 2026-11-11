# Upstream review: 2026-11-11

The mandate pins remain the behavioral baseline. This review found compatible MCP
security/correctness fixes, current immutable CI action pins, and specific editor
and FFmpeg regressions to include in the Rust implementation. It does not qualify
a release, replace the source lock, or import donor runtimes.

The machine-readable companion,
[upstream-sources-2026-11-11.json](upstream-sources-2026-11-11.json), records full
baseline/head/release identities and comparison URLs. Observations were collected
on 2026-11-11, through 11:39 UTC, against the implementation based on
`4d2ca7de44071103db8bca0795b7d6d6adba79ff`.

## Evidence and scope

Read first: `.ai/memory.md`, root `AGENTS.md`, the local constitution, specification
and plan, task headings, and the mandate's source/change policy. Current user
instructions supersede earlier branch restrictions. The cloud runtime/network
skills were read; GitHub connector calls supplied repository evidence. A live
Firecrawl read of [FFmpeg's official download page](https://ffmpeg.org/download.html)
established the current stable release. No additional donor clone was needed.

The review compared exact source pins with observed default heads, read relevant
actual PR/commit patches, and checked current Rust source/feature declarations.
Release notes selected leads; they were not treated as proof of a patch's scope.
GitHub caps a compare response at 300 changed files. FilmCraft, EffectCraft,
FFmpeg master and egui reached that cap; their review is targeted, not an exhaustive
new donor catalog. No application build, ordinary tests, FFmpeg media run or owner
GPU qualification was performed locally.

## Observed source identities

Each short hash below is only for readability; full hashes are in the companion.
A release can predate the research date and still be newer than its selected pin.

| Source | Mandate baseline | Observed default head | Latest stable release | Result |
| --- | --- | --- | --- | --- |
| FFmWiz | `661a0d240ee2` | `661a0d240ee2` | latest-release endpoint 404 | Unchanged. |
| MuxCls | `0cf9596f7510` | `0cf9596f7510` | latest-release endpoint 404 | Unchanged. |
| FilmCraft | `d643a886650b` | `db7fa4d0efe3` | `v0.19.0`, `d1af238bba9f` | 1670 commits ahead; targeted editor deltas below. |
| EffectCraft | `9db7f3b21700` | `3d1b6f4f0b22` | `v0.23.1`, `f5e5c8a4d8e0` | 2305 commits ahead; bounded preview scheduling evidence. |
| Vidonex | `ae04a3a4e7e4` | `ae04a3a4e7e4` | `v1.6.0`, same commit | Unchanged. |
| ffmpeg-sidecar | `958985bfd064` | `958985bfd064` | `v2.7.0`, same commit | Unchanged; no new runtime dependency needed. |
| Shotcut | `2b943b01b5b5` | `7a025adb1e86` | `v26.9.27`, `325c2cc6422d` | 32 commits ahead; exact snapping and subtitle fixes. |
| Av1an | `50f9ea6b6867` | `641ce62d70e3` | `v0.6.0`, same commit | Exactly two new commits; stdin isolation is the runtime delta. |
| LosslessCut | `9a38272d36b2` | `4c7ab1cb0c9a` | `v3.70.2`, `2610284b6c9a` | 154 commits ahead; media CLI/parser/extraction deltas below. |
| FFmpeg | `n9.0.2`, `946fcce07b6d` | `9bee1e603cdb` | `n9.0.2`, same baseline commit | Stable unchanged; maintenance branch has 49 additional commits. |
| Rust | `1.99.1`, `58cf8f9c35b3` | `9de746938314` | `1.99.1`, same baseline commit | Retain stable toolchain. |
| egui/eframe | `0.36.2`, `49682f8baa05` | `38f6bfa3d680` | `0.37.0`, `841a328ba2dd` | Breaking UI/features; separate migration gate. |
| rmcp | `3.6.0`, `74d789515fde` | `f2b0ddf113d1` | `3.9.0`, `5596a8a0bba2` | Compatible source update selected for integration. |
| redb | `4.4.0`, `7061f7e60d45` | `1ea02d9ab4a9` | `4.5.0`, `96ce31fe5caf` | Keep single-owner policy; qualify storage upgrade separately. |
| FFmpeg Skill | No pin in baseline lock | `532cbe38cbcc` | `v3.4.3`, `32e3355795d5` | Supplied checkout already at head; no invented earlier baseline. |

The FFmpeg and egui default heads diverge from their release pins. Their
ahead/behind counts are branch ancestry, not a supported upgrade path. Default
heads can advance after observation.

## Integration decisions

### UR01: adopt the compatible rmcp maintenance update

Change the workspace dependency to `rmcp = "=3.9.0"`, resolve its lockfile in
hosted CI, and bind the HTTP SDK config to
`.with_cancellation_token(listener_stop.child_token())` in
`apps/numera-wiz/src/server.rs::router`. Accepted durable jobs retain their own
existing lifetime; closing a listener must terminate its sessions without
implicitly cancelling an accepted job.

The actual [3.6.0 to 3.9.0 diff](https://github.com/modelcontextprotocol/rust-sdk/compare/rmcp-v3.6.0...rmcp-v3.9.0)
contains the stateful-session cancellation fix
[`8878a791df74`](https://github.com/modelcontextprotocol/rust-sdk/commit/8878a791df74746c2dffb7b8bba9d39c64cce8d2),
exact HTTP media-type and Accept quality parsing
[`c60e824dadb4`](https://github.com/modelcontextprotocol/rust-sdk/commit/c60e824dadb49dde6dc0993132e6b9a0f1dec71a),
invalid `Mcp-Param` rejection
[`0413771a9635`](https://github.com/modelcontextprotocol/rust-sdk/commit/0413771a963599ef6cfcece8e9ba55e069e39005),
and preservation of protocol error replies during cancellation
[`9e0783d00e9f`](https://github.com/modelcontextprotocol/rust-sdk/commit/9e0783d00e9f51f70fab2e7507dfecc55cb974fe).

Exact release source inspection confirmed:

- Edition 2024 and MSRV 1.88 fit the pinned Rust 1.99.1 toolchain.
- Existing server, transport-io, transport-streamable-http-server and schemars
  feature names remain. Existing test client/child-process/HTTP feature names remain.
- `ErrorData`, `RoleServer`, `ServerHandler`, `ServiceExt` and `RequestContext`
  remain available; `ServerConfig` remains the `InitializeResult` alias.
- Current handler signatures, `Tool::new`, protocol constants,
  `CallToolResponse`/`ReadResourceResponse` and their result conversions remain.
  The latter are defined in `model/mrtr.rs`.
- The release does not require adopting its new Skills extension or changing
  Numera's command authorization. Current code has no `McpServer` type to rename.

Extend existing hosted transport assertions for both initialize/discover profiles,
exact Content-Type, split Accept headers, zero-quality media types, malformed
Mcp-Param values, listener shutdown and accepted-job survival. Preserve the existing
test categories and test-function count.

Working-tree integration was subsequently observed: the workspace now pins 3.9.0
and the router binds the listener's child cancellation token. The selected
dependency closure and these behavioral assertions still require hosted evidence.

### UR02: update CI actions with their actual new contracts

| Action | Verified tag | Immutable commit |
| --- | --- | --- |
| checkout | `v7.0.1` | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| upload-artifact | `v7.0.2` | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |
| download-artifact | `v8.0.2` | `9000827ccba6bdab643e8b6fd33ac0654aef8333` |
| cache | `v6.1.0` | `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` |

Sources are the exact tag refs and release URLs in the companion. Node 24 requires
Actions Runner 2.327.1 or newer; authenticated checkout operations from Docker
container actions require 2.329.0 or newer. Checkout v7 refuses unsafe fork PR
checkout on `pull_request_target`/`workflow_run` by default; preserve that behavior.

[Upload v7](https://github.com/actions/upload-artifact/releases/tag/v7.0.0) and
[download v8](https://github.com/actions/download-artifact/releases/tag/v8.0.0)
use ESM. Upload's `archive: false` accepts one file and takes the artifact name from
that filename; it ignores the supplied `name`. Keep normal zipped behavior for
directory artifacts unless the consumer is deliberately changed. Artifact names
remain immutable and must be unique per matrix output. Hidden files are excluded
by default. Zip upload loses executable modes; package as tar before upload where
modes matter.

Download v8 fails digest mismatches by default; retain that check. It decides
whether to unzip using Content-Type; `skip-decompress` is explicit. Cache v6 uses
ESM/toolkit major updates. Its 6.1.0 read-only save denial warns without failing the
job; a successful cache step therefore does not prove a cache was written.
Hosted workflow evidence is still required after changing pins.

### UR03: keep the redb ownership contract during a separate upgrade

[4.5.0](https://github.com/cberner/redb/releases/tag/v4.5.0) adds stable
`ConcurrencyMode`, explicit close results, nonblocking write acquisition and
integrity-on-open options. It also fixes corruption paths that previously
panicked. Its new shared modes cannot safely share a file with processes using
4.3/4.4 experimental multiprocess locking. The current Numera workspace enables
no experimental multiprocess feature and holds its own `RootLock` before opening
redb in `crates/nw-store/src/lib.rs`.

Do not turn this into multiple native owners or browser direct database access.
A storage update must retain the root lock and ExclusiveWriter behavior, exercise
existing migration/backup/rollback/crash fixtures against real 4.4 databases, and
handle explicit close failures if adopting the new API. The observed master
read-only repair/lock fix is unreleased and concerns shared-reader semantics; it
does not by itself justify a git-head dependency.

### UR04: qualify egui 0.37 separately

[0.37.0](https://github.com/emilk/egui/releases/tag/0.37.0) improves accessibility,
font fallback, keyboard conflict handling and native/window behavior, but changes
font features, panel ID salts, screenshot callbacks, widget atom APIs and roles.
The workspace deliberately disables defaults and enables `default_fonts` plus
Glow; native adds X11/Wayland. Keep that explicit backend choice during a migration.
Do not import donor wgpu renderers or assume system-font lookup qualifies media
font output.

Audit `nw-ui` panel/ID/focus calls and web drop/clipboard feature use, then compare
native/browser focus, text entry, shortcuts, image handling and screenshots under
the existing CI UI journeys. Current `0.36.2` remains the selected integration pin
until that coordinated migration passes.

### UR05: carry current FilmCraft behavior into the Rust editor

Actual patches, not just release titles, establish these requirements:

- [#1413](https://github.com/storytold/filmcraft/pull/1413),
  merge `f5cfd09f9ee1`: command descriptors declare accepted named target keys.
  Numera's `nw-domain::commands::Descriptor` and editing DTOs should expose typed
  explicit targets and validate them against the command domain and revision.
  Preserve Numera's strict schemas; do not port donor parameter-doc string parsing.
- [#1400](https://github.com/storytold/filmcraft/pull/1400),
  merge `73a60569aa07`: Effect Controls consumes Clear/Ripple Delete. Remove the
  selected removable effect/mask on a currently shown clip; intrinsic effects,
  stale selections and repeated Backspace must not fall through to clip deletion.
- [#1962](https://github.com/storytold/filmcraft/pull/1962),
  merge `3dde4f20cc92`: box selection/Select All include captions. Mixed
  clip/caption deletion forms one undo transaction, respects locked tracks and
  does not delete/ripple a caption twice after the clip edit already removed it.
- [#4306](https://github.com/storytold/filmcraft/pull/4306),
  merge `4a273b868a32`: Up/Down inside a numeric text field changes that field,
  including modifier steps, without moving the timeline playhead.
- [#4541](https://github.com/storytold/filmcraft/pull/4541),
  merge `e9362f9476c1`: LUT export targets the clip displayed by the panel and
  reports effects omitted from the LUT. The FFmpeg-backed grade/export contract
  must make those limits visible instead of implying full visible-grade identity.

Current `nw-ui` primarily implements cleanup/job controls; these are integration
requirements for the unfinished editor, not claims that those workflows exist.
Add exact source-to-Rust/CI links as each implementation lands.

### UR06: use Shotcut's actual edit/parser regressions

[Low-zoom snapping](https://github.com/mltframework/shotcut/commit/0d30d391a8fc9a68c1c82b3651afa4c5c069a6e3)
replaces rounded display coordinates with frame-derived playhead/marker
coordinates. Keep snap decisions in Numera's exact-time domain and round only the
presentation; include zoomed-out boundary cases in existing timeline assertions.

[Subtitle parsing](https://github.com/mltframework/shotcut/commit/1429e92fc9aa91b52f35d244edabf7cf59a7ee4d)
normalizes CRLF, ignores extra blank separators and avoids dropping later cues.
Apply that behavior to Rust caption import; preserve the actual caption text.
The diff also fixes multi-dissolve paste shifts and cross-track drag placeholders.
These are behavioral test leads, not permission to add Qt/MLT or donor DSP.

### UR07: bound preview work, not only retained frames

EffectCraft [#3047](https://github.com/storytold/effectcraft/pull/3047) adds a
two-frame GPU render gate; [#3355](https://github.com/storytold/effectcraft/pull/3355)
then adapts preview concurrency to observed window stalls. Their source shows why a
cache-byte limit alone does not bound simultaneous working sets.
[#3336](https://github.com/storytold/effectcraft/pull/3336) avoids repeated footage
uploads and chooses a job renderer from measured timings without mixing renderers
within the job.

In Numera, implement these principles in the FFmpeg preview owner: bounded
in-flight requests/bytes, playhead priority, stale-generation rejection, explicit
backpressure and a single frozen admitted route per job. The existing
`nw-engine::budget::ProcessBudget` caps leaf processes at two; that is not yet
a complete frame/cache/GPU budget. Do not port EffectCraft shaders, wgpu
compositors, Condvar waits into async handlers, or its performance numbers.
Actual GPU responsiveness/memory thresholds need the matching owner harness.

### UR08: retain stable FFmpeg provenance and qualify maintenance fixes

The official stable remains n9.0.2. The
[release/9.0 comparison](https://github.com/FFmpeg/FFmpeg/compare/946fcce07b6dcd0331c8cc609192aeff5e1924f8...c179411bbb2864cc71323d3055acda25d5c2aace)
has 49 commits and 49 changed files. Relevant changes include HLS byte ranges
(`2120a1d6266d`), MOV allocation cleanup (`c867e135494d`), trailer I/O error
propagation (`9695f0c831d4`), Matroska duration overflow (`0ba284bbcbbe`),
CUDA scaling decisions and Vulkan/VideoToolbox lifetime fixes.

These justify a tracked component rebuild candidate with its own exact source,
patch/configuration/license closure and platform preservation fixtures. Do not
label a maintenance-head binary as the unmodified stable tag. The comparison
changes no RTSP source; it provides no new evidence that the known redirect-loop
route is fixed. Keep unqualified network/secondary-open routes blocked. Current
`nw-engine::admission` permits narrowly identified regular files and rejects
external MOV references; broader source import needs its own resolver and tests.

A source/version match alone does not qualify packaged FFmpeg/ffprobe, codecs,
devices, child-open authority or the selected GPU route.

### UR09: preserve the small Av1an delta and LosslessCut edge cases

Av1an's [exact two-commit diff](https://github.com/rust-av/Av1an/compare/50f9ea6b6867288c1c287ec9a6787ec708fb8fb2...641ce62d70e34f438f3ee91ec50de77784a10705)
adds null stdin to spawned source/metric/quality pipes, then releases 0.6.0.
Numera's ordinary supervisor already selects `Stdio::null()`; preserve that
default and use only explicit reviewed pipe inputs. The 0.6.0 release notes
describe many changes already present at the mandate pin; do not count them all
as newly discovered deltas.

The [LosslessCut diff](https://github.com/mifi/lossless-cut/compare/9a38272d36b249df2ffcdb65b4b4d42101953735...4c7ab1cb0c9acaefabb48e997bc909562dc3624d)
establishes:

- FFmpeg 9 frame extraction uses `-fps_mode passthrough` instead of removed
  `-vsync 0`. Apply that to new extraction lowering.
- FLAC export sets an explicit 4608 frame size to avoid inheriting an unsuitable
  input frame size; qualify per-output-stream options on the actual profile.
- Black/silence diagnostic prefixes differ on Windows (no `0x`, uppercase hex).
  New analysis parsers must accept admitted platform forms, stay bounded and
  report failed/absent measurement honestly. Current `progress.rs` parses the
  separate machine-readable progress channel and does not need that regex.
- [Attachment extraction](https://github.com/mifi/lossless-cut/commit/9837beddcea3db507392f2fb88ce36267f53e7d5)
  can write outputs then return 1 when no ordinary output is specified. Give
  Numera an explicit extraction outcome contract and verify every owned staging
  artifact; do not generalize stderr text into a success oracle.
- [Attachment filename suffixes](https://github.com/mifi/lossless-cut/commit/4a298e3235bb16864a45c8ee38bc98ef60790faf)
  admit only a bounded plain extension. Treat embedded names as untrusted data;
  preserve Numera's native-path grants, ownership and destination checks.

### UR10: use FFmpeg Skill as behavioral evidence

The reviewed [skill tree](https://github.com/kajisho5/ffmpeg-skill/tree/532cbe38cbcc018f45c5f400e570abd82b37fc93)
is a Python 3.10+/FFmpeg 6.1+ toolset, not a Rust runtime dependency. Its
`SKILL.md`, contract and focused design decisions distinguish measurement from
writes, retain output identity, report unmeasured assertions, preserve one common
audio/video clock and support declared changed-range comparisons.

Use those ideas in the Rust planning/verification contract and agent guidance.
Do not include Python, its MCP server, tool-count claims or skill-specific timeout
environment settings in the product. Numera's typed protected-owner disabled
deadline policy and strict scoped callers remain authoritative. Latest 3.4.3
release work concerns plugin listing/privacy metadata; no prior skill pin exists
in the mandate lock from which to claim a complete behavioral delta.

## Evidence still needed

Hosted CI must resolve the selected SDK closure, compile every native/WASM target,
run existing protocol/storage/media/UI categories, and retain exact source and
component hashes. Changes in this document are recommendations until linked to
actual implementation and passing hosted checks.

The full donor inventory must eventually account for newly selected features and
intentional exclusions; this targeted update does not replace the mandate's
pinned coverage CSVs. The root integration must update current continuation and
task records without claiming catalogs, schema counts or source translation prove
user workflows.

Owner hardware remains necessary for matching-candidate GPU route correctness,
fallback, cancellation, preview responsiveness, memory ceilings and measured
performance. Portable FFmpeg/ffprobe packages, same-build browser assets, offline
first-use and source/license closure need actual per-platform package evidence.
None can be inferred from an upstream release, a successful source read, or a
hosted CPU build.
