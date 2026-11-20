> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## 8. Resources, security, distribution and CI

### Resource budgets, security and distribution

#### Lightweight behavior is an engineering contract

Use streaming, demand-driven work and bounded queues. Start with one heavy export and one active preview, admitted only when their combined resource estimates fit the selected budget. A CPU core count is not a concurrency policy. Probe/thumbnail/waveform tasks share a bounded scheduler and yield to interactive requests. Do not preload the effects catalogue's models, all fonts, every media frame or entire audio tracks into memory.

Adopt these initial budgets and measure them in the first packaged vertical slice. They are design targets, subject to evidence-based refinement while preserving the user's lightweight goal:

| Resource | Initial policy |
|---|---|
| redb cache | Explicit 8 MiB; do not inherit an unexpectedly large library default. |
| Managed image/thumbnail cache | 128 MiB total maximum, configurable downward; disk cache separately limited and content-addressed. |
| In-memory undo | 32 MiB starting budget with reversible patches/structural sharing; retained job snapshots are separately accounted. |
| Native decoded video queues | At most 3 pending frames per owned queue, with explicit ownership/copy accounting; normal interactive preview is 720p. |
| Rendered audition audio ring | Initial maximum 80 ms; observe device/OS buffers and underruns separately. |
| Scrubbing | One active plus one latest pending request; obsolete generations are cancelled/dropped. |
| Browser media | Bound both pending bytes and buffered time; initial 4 s lookahead, aggressively evicted behind playhead, explicit stalled-client deadline. |
| Commands/events | Default JSON command limit 1 MiB; larger projects/assets use separately bounded streaming routes. Event ring starts at 8 MiB, paginated results at 256 KiB. |
| Logs | Initial 5 × 10 MiB rotated files, age cap 14 days, plus a bounded current-stage tail; essential execution records persist with the job. |
| Heavy work | Default total job budget selected from available memory with OS/application headroom; begin with a 1 GiB ordinary-processing target and explicitly higher/lower profiles. Preflight rejects an impossible route. |
| Idle processes | Worker exits after the specified lease-free grace; no idle video decoding, continuous UI repaint loop or background model scan. |

A 720p packed RGBA frame is 3,686,400 bytes; a 4K frame is 33,177,600 bytes. Three-frame queues, textures, packet buffers, decoder surfaces and filter history can all coexist. Count actual allocations and process RSS, not only one application cache. GPU memory, disk scratch and browser-process memory are separate columns in the report; unknown telemetry is not zero.

Initial performance targets on a recorded reference environment: no-job application/worker steady-state memory at most 150 MiB; ordinary 720p preview process-tree memory at most 750 MiB; warm GUI startup within 2 s; cached/proxy seek p95 within 300 ms; responsive cancel acknowledgment within 250 ms before graceful subprocess shutdown. First extraction, cold original-media seeks and expensive filters are measured separately. These values are not promises for arbitrary media or hardware. If a target fails, identify the component, tune or choose a better valid route and document any revised engineering target; do not hide a failure by removing required baseline capabilities.

FFmpeg also owns memory. Bound thread counts, decoder surfaces, filter graphs, lookahead, packet queues and muxer accumulation. Reverse, temporal denoise, thumbnail batches, floodfill, geq prefix sums, large IRs and model runtimes have internal storage. For example, upstream `thumbnail` retains its batch, default 100; scale-after-selection does not reduce the retained full-resolution batch. Use a bounded selection strategy, a declared scale-first approximation or targeted seeks. Never expose upstream maximum radius/search/order as a claim that all such values fit the product's resource budget.

Memory estimates must include arithmetic overflow checks, overhead and uncertainty. Monitor actual process-tree memory and scratch consumption, provide pressure feedback, stop lower-priority prefetch, shrink previews/caches and cancel safely before exhausting the host where possible. Use qualified OS limits for strong process containment; do not claim a sampling watchdog is a hard instantaneous memory cap.

Benchmark idle/startup, import of large bins, long timeline navigation, warm/cold seeking, thumbnail/waveform generation, one common CPU transcode, stream copy, effect-heavy render, constrained-memory reverse, batch concurrency and browser/native coexistence. Measure quality and preserved semantics alongside speed. Keep artifacts, fixture hashes, hardware/driver, profile, p50/p95 latency, throughput, RSS/VRAM/scratch and regression baselines. Shared hosted runners establish functional/regression evidence; absolute claims need a stable recorded environment.

#### Repeatable lightweight/performance measurements

Keep the existing initial targets: 150 MiB no-job application/owner steady state; 750 MiB ordinary 720p preview process-tree memory; 2 s warm GUI startup; 300 ms cached/proxy seek p95; 250 ms cancel acknowledgement before graceful child shutdown. These remain engineering targets, not universal hardware guarantees. Define the following measurement protocol before comparing releases.

The conservative primary resident-memory total is the sum of RSS on qualified Unix targets or working-set bytes on Windows for the application, worker and all owned media/helper processes. Shared pages can be counted more than once; label that fact and do not silently switch to private memory to obtain a pass. Record available private/commit/PSS or platform footprint metrics in separate columns. Browser measurements use a clean dedicated profile/process group and report the full browser footprint plus the local engine separately; they do not include an unrelated user's existing tabs or present an unknown browser cost as zero. Record VRAM per process when available and whole-device telemetry separately when attribution is unavailable.

Freeze a small `benchmarks/workloads.json`: empty project idle; a specified 720p playback fixture; repeated cached/proxy seeks; large-bin/timeline navigation; stream copy; one common CPU transcode; and one bounded effect-heavy graph. Reuse media correctness fixtures where suitable. Each entry fixes file/graph hashes, dimensions/rate/duration/streams, preview policy, cache state, thread/concurrency/resource configuration and success assertions. Add a workload only when it represents a required user task or a concrete regression risk.

For idle measurement, wait until the app reports no pending initialization/probe/render work, then sample at a fixed recorded interval for at least 60 s. Separate cold first extraction from warm startup. Startup ends when the first usable editor frame and command service readiness are both observed, not merely at process creation. Seek latency ends at the displayed frame matching the requested project generation/time; a loading placeholder is not completion. Cancel latency ends at the command acknowledgement; child termination/cleanup is a separately recorded duration. Collect at least 40 completed warm/cached seek samples for the reported p95, with explicit deterministic seek positions and seeds. Count failures/timeouts separately instead of dropping them from the sample.

Peak/steady metrics, CPU time, throughput, scratch bytes and quality assertions accompany latency. Record the runner/hardware, OS, browser/driver, power mode where observable, artifact hashes and cache reset method. Hosted shared runners establish reproducible procedure and broad regressions; noisy absolute latency cannot be claimed as stable owner-hardware performance. The local owner bundle reuses the same workload definitions for its permitted GPU/desktop checks. A failed target leads to a measured optimization or an explicit reviewed target revision, never to dropping required workflows or reclassifying unmeasured memory as zero.

**D6-J08 — Resource evidence journey (WP10):** run the frozen idle/preview/seek/cancel workloads through the packaged artifact, observe the complete owned process tree and separate browser/VRAM/scratch metrics, and compare like cache states and timing endpoints. A deliberate helper-memory increase, stale-frame seek acknowledgement or missing telemetry must be visible in the report instead of producing a false target pass.

#### Security boundaries that require real tests

Apply Section 5's object-action policy to all read projections and transfers. Section 6 selects input admission before the first probe; Section 7 selects the packaged browser CSP/MIME and executable asset namespace. A path grant or protocol whitelist alone does not close these boundaries.

Use scoped read/write/device/network grants. A CLI path explicitly supplied under the local-owner principal can establish its requested operation's grant. This rule does not apply to a stdio bridge or an agent request merely because its transport is a local process: the selected access profile remains the effective principal. A paired browser/MCP token does not inherit unrestricted filesystem access. Native GUI file/directory selection grants the selected assets/roots. Browser sessions opened from that GUI can receive the selected project/asset grants. A headless `serve`/token setup supports explicit allowed roots. Without a suitable root, the browser can stream a chosen upload into a quota-limited owned staging area; it cannot discover the original OS path from a browser file object or require a whole-file RAM copy.

All served file/media URLs use opaque authorized handles. Canonicalize native paths without lossy identity conversion; validate symlink/reparse/junction behavior, UNC/network paths, directory traversal, source/output aliases and overwrite claims. A portable project file containing an arbitrary absolute path does not grant access to that path. Reopening/relinking expands grants only through the intended local user/agent policy.

Control FFmpeg's secondary opens: concat lists, subtitles/fonts, LUTs, IRs, model files, movie/amovie, manifests, filter scripts, protocols and referenced network resources. Use typed graph nodes, safe private sidecars and protocol restrictions appropriate to the plan. Local media work has no undeclared network fetch. A URL source is a distinct explicitly authorized capability with redirect, credential, timeout, size and SSRF policy. Reject shell execution and unsafe expert options; validate aliases and file-loaded option forms, not just an option-name denylist.

Browser/API security requires exact Host/Origin validation, no wildcard CORS, one-use pairing, explicit bearer headers, bounded session/requests, no secrets in URLs/logs and a strict CSP with only packaged assets. Do not load third-party scripts, render untrusted project metadata as HTML or follow authenticated redirects to another origin. Static UI assets may be public on loopback; project/media/service data are authenticated. Local loopback is not an authentication mechanism. A compromised same-user account is outside the isolation guarantee; state that boundary accurately.

Component/archive import validates manifest signatures where available, pinned hashes, size/count limits, path normalization, symlinks, duplicate/case-colliding entries and decompression ratios. Extract into an owned staging directory under a lock and publish only after validation. Never overwrite arbitrary user files or accept an unsigned optional pack as silently trusted code. No automatic execution of a donor's scripts/plugins from a project.

Restrict first-party `unsafe` to small reviewed platform/FFI boundaries with written invariants; forbid it in the pure domain model where practical. Fuzz exposed parsers and state machines. Native FFmpeg and codec dependencies need their own update and vulnerability process; Rust memory safety does not erase their attack surface. Sandboxing is a platform capability that must be exercised before it is advertised, not an unimplemented checklist word.

#### FFmpeg component profiles and licensing

Ship one coherent `standard` FFmpeg/ffprobe profile per platform with all required ordinary CPU functions. Select a GPL-compatible build (`--enable-gpl --enable-version3`) and refuse a release profile whose actual configure/license result is nonfree/unredistributable. Never patch out upstream checks. First-party licensing, third-party code licenses, fonts/models/assets and distribution obligations are tracked separately.

The standard profile includes the required native codecs/filters/muxers/demuxers/protocols/bitstream filters and a deliberate dependency closure:

| Functional need | Standard profile decision |
|---|---|
| H.264 / HEVC software encoding | Include qualified libx264/libx265 depth/profile variants. |
| AV1 | Include dav1d decode and one principal software encoder, initially SVT-AV1, qualified on x64 and ARM64; do not bundle every AV1 implementation without need. |
| VP8/VP9, MP3, Opus, AAC, FLAC, PCM | Include libvpx, libmp3lame, libopus and appropriate native FFmpeg routes; retain FFmWiz export formats. |
| Lossless/intermediate/image work | Required FFV1, PNG/image sequences, rawvideo, PCM/FLAC and compatible containers. |
| Text/subtitles | libass, libunibreak, FreeType, HarfBuzz, FriBidi and deliberate platform font providers; qualify `wrap_unicode` against the actual libass API/build and preserve its native-ASS default difference. Include fontconfig when using drawtext family/fallback resolution, or resolve explicit font files. A small licensed font set works offline. |
| Color/HDR | zimg/zscale and admitted native color/tonemap/LUT filters. |
| Pitch/time | Rubber Band through FFmpeg, including its exact chosen native dependency closure. |
| Stabilization | libvidstab where the admitted workflow uses it. |
| Vector graphics | Cairo/pixman with FFmpeg's actual `--enable-cairo` switch for admitted drawvg operations. |
| Hardware wrappers | Platform-appropriate NVENC/NVDEC, QSV/VAAPI/AMF/VideoToolbox only where their dependency profile remains compatible with CPU-only startup. |

Generate the required feature closure from the capability registry before aggressive FFmpeg pruning. Do not remove subtitle encoders, parsers, lavfi sources or muxers just because a size script only counted video encoders. Choose shared-private versus static native linkage per target with actual import/size evidence; both remain embedded portable component contents, not extra user installation steps.

E02 establishes separate compressed/uncompressed size baselines for first-party native code, browser WASM/glue, fonts/assets and each FFmpeg/dependency profile. CI reports each part, catches duplicate runtimes and requires an explained review for growth above 5 percent or 5 MiB in a release artifact. These are regression triggers, not reasons to remove required functionality. Optional model/asset profiles are measured separately and never included in ordinary startup memory or the standard EXE by accident.

Large speech/DNN models, uncommon GPU compute dependencies, OCIO/other specialized processing when substantial, and licensed IR/HRTF packs can be explicit optional component profiles. Each admitted optional feature ships a real installable/offline-importable pack or a documented user-asset contract. Its absence produces a precise missing-component state on every surface and preserves project data. Do not hide normal core codecs behind unexplained downloads or fill the editor with permanently unfinished panels.

Adding a DLL beside an existing FFmpeg binary does not register a newly compiled filter. Optional profiles are complete compatible executable/dependency sets unless actual dynamic loading is audited. Missing optional libraries must not prevent the standard CPU binary from starting.

For this FFmpeg pin, distinguish ordinary NVIDIA codec APIs from NVCC/proprietary dependency paths. The researched configure supports LLVM/Clang CUDA compilation, marks NVCC paths differently and removes the old libnpp functionality. Check exact source/profile licenses rather than copying a tutorial's `--enable-nonfree`. The official `libvmaf_cuda` example itself uses nonfree flags; it cannot be copied into the public GPL release profile. Keep an eligible CPU metric route. Driver/API version minima come from the exact selected headers/runtime; test the owner's actual driver instead of inferring compatibility from GPU marketing names.

Preserve FFmWiz/FilmCraft/EffectCraft/Vidonex and other reused-code notices according to their actual terms. Use Numera branding; code reuse does not transfer donor trademarks or every bundled font/stock asset. Publish GPL license text, third-party notices, SBOM, corresponding source and exact build recipes for redistributed binaries. Do not label a package legally cleared merely because a license scanner returned green. [FFmpeg licensing](https://ffmpeg.org/legal.html) · [Pinned configure](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/configure)

#### Machine-readable component and distribution closure

E02 must emit a versioned `component-profile.json` and `capability-closure.json` from the actual built package. The source lock used for research is not a substitute for these build outputs.

| Record | Required fields |
|---|---|
| Profile identity | Schema version, platform/architecture and minimum OS, profile ID/digest, FFmpeg/ffprobe artifact digests, applicable application/registry compatibility. |
| Build inputs | Exact upstream revisions/archive hashes, toolchain/SDK identity, configure/build options, applied patches and source/build recipe locations. |
| Runtime closure | Every packaged executable/shared library/font/data asset, size/digest/license record, native imports, permitted OS-provided libraries and optional driver dependencies. |
| Actual features | Captured configure/version/license output; encoders, decoders, muxers, demuxers, filters, bitstream filters, protocols/devices, required generated help and known format/parameter restrictions. |
| Capability closure | Canonical capability/variant, required profile items, actual feature match, fixture/evidence IDs and precise missing requirement. |
| Distribution | Compressed/uncompressed sizes, required notices/licenses, corresponding-source archive and build-recipe digests, signing/provenance status. |

Generate the required side from the registry and compare it with the built side. A required ordinary codec/filter that fails to build is a failed target profile, not silently an optional capability. A specialized optional component remains optional only under its explicit capability policy and must have the promised install/import path. No untested filter/format combination becomes available merely because its name appears in `-filters`.

Publish separate files for each native target, browser assets embedded with that target, the owner qualification bundle, checksums, provenance/attestation material, third-party notices/SBOM and corresponding source/build recipes. The initial release names the four native artifacts unambiguously: Windows x64 EXE, Linux x64 portable archive, macOS x64 bundle archive and macOS ARM64 bundle archive. CI logs and a GitHub source-code ZIP alone do not constitute the complete redistribution/source record. The actual chosen dependency licenses and configuration remain authoritative; this specification is not a legal-clearance certificate.

**D6-J05 — Profile closure journey (WP02):** build one profile per required target, compare registry requirements against actual binary capabilities/imports and execute the declared representative route fixtures. Deliberately remove one required filter, one private library and one notice/source entry in CI package fixtures: each mismatch blocks the appropriate availability/distribution gate, and absence is not reclassified as an optional feature.

#### Portable artifacts and platform scope

Initial supported/qualified targets are Windows 11 x64, Ubuntu 24.04 x64, and macOS 15 Intel/ARM64. These are selected support baselines, not claimed completed tests. Broader Windows/Linux/macOS compatibility is documented only after actual import/runtime evidence. Keep target-specific code behind adapters; reserve Android lifecycle, storage grants, touch layout, background job, thermal/power and MediaCodec/FFmpeg policy without adding an unbuilt Android promise to the current release.

Windows download is one `numera-wiz.exe` containing the application, web assets, standard compressed FFmpeg/ffprobe runtime, required fonts and notices. First launch validates and extracts helpers under an owned versioned portable root, then uses them directly. The one-file distribution does not imply in-memory execution of arbitrary embedded executables. There is no core first-launch network download or installer requirement.

Choose the data root in this order: explicit `--data-dir`; a deliberate portable-root setting; the writable directory beside the executable. If it is unwritable, show a concrete chooser/error rather than silently abandoning portability for hidden AppData. If `G:\Program Files` is protected, the user can place the EXE in a writable portable folder or select an explicit data root. Never require elevation for ordinary media work. User-scoped OS runtime locks/credentials needed for cross-root project safety are transient and documented; they are not a hidden media/cache installation.

Use a GUI-subsystem executable for double-click without a console window. Preserve inherited valid stdio handles and qualify PowerShell/cmd/process-spawn/MCP exit and pipe behavior early. Worker mode is the same executable with no GUI initialization. Linux ships a portable tar archive with declared native imports; an optional AppImage cannot make FUSE a core requirement. macOS ships separate Intel/ARM64 app bundles with CLI/bridge entry instructions; mutable data/components stay outside signed bundles. Report real signing/notarization status. Missing credentials means unsigned/unnotarized, not a fake successful gate.

Updates are explicit, checksum/signature-validated and rollback-capable. Do not mutate a profile used by accepted jobs. Keep old profiles while referenced, then garbage-collect only owned obsolete contents. Interrupted extraction/update must retain the last usable version. Uninstall removes only manifest-owned runtime/cache files under the chosen policy; never delete source media, projects or exports by a broad recursive cleanup.

#### Portable first launch, owned storage and repair

The Windows artifact remains one `numera-wiz.exe`, usable offline without a compiler, Node, Python or an installer. Embedded helpers may be extracted. Resolve the explicitly selected portable data root before opening the durable store, provisioning credentials or starting a listener. The existing precedence remains `--data-dir`, a deliberate portable-root setting, then the writable directory beside the executable. Evaluate permissions and required filesystem operations, not only whether a test file can be created.

Use this logical layout under the chosen root; all paths are discoverable in `doctor` and configurable only through the existing typed configuration policy:

| Owned path | Contents and lifetime |
|---|---|
| `state/root.json` | Data-root UUID, active durable-store generation, application/store format compatibility and activation marker. No bearer secrets. |
| `state/generations/<generation>/` | Durable project journals, unsaved edits, receipts, jobs and access-policy records. Retained generations have the migration/rollback rules below. |
| `state/auth/` | Protected principal/credential material and replay verification keys. Explicit token exports may use another validated user-only path. |
| `assets/<content-digest>/` | Immutable verified managed imports, captured takes and generated retained media, with separately stored ownership/reference records. Retained media is not ordinary cache. |
| `components/<profile-digest>/` | Immutable verified FFmpeg/ffprobe and private dependencies, fonts/assets and profile manifest. A published profile is not updated in place. |
| `cache/` | Rebuildable thumbnails, waveforms, proxy indexes, UI/font caches and other admitted owned cache. A generated proxy retained by a project/job has a live reference and is not disposable merely because it is under a cache root. |
| `scratch/<session-or-job-id>/` | Bounded temporary sidecars, intermediate media and child-process temporary files. Publication staging beside the target is recorded separately when required for same-filesystem publication. |
| `logs/` | Rotated timestamped diagnostics under the existing byte/age caps. |
| `staging/` | Incomplete component extraction/import and update preparation. A verified activation marker distinguishes ready data from an abandoned attempt. |

Source media, external projects and exports remain outside blanket cleanup ownership even when a user deliberately locates them below the same root. Imported browser assets and captured takes are retained media, not cache, while referenced or explicitly retained. A reset/forget-data operation lists those distinctions and never turns a broad recursive directory deletion into the default uninstall path.

For each controlled native child, create an explicit environment block. Point `TEMP`, `TMP` and `TMPDIR` at its admitted owned scratch location and `XDG_CACHE_HOME` at the owned cache location. Qualify relevant font/cache variables for the selected profile. Do not rewrite the user's machine/user environment. Do not overwrite `HOME`/`USERPROFILE` to achieve portability. An inherited `FFREPORT` or equivalent diagnostic override must not silently redirect FFmpeg logs into an arbitrary path; use the application's explicit diagnostics policy. Missing space, an unwritable scratch root or an unavailable execution mount is a visible error or an explicitly selected alternative root, not an automatic system-temp fallback. OS, browser and GPU-driver caches remain outside this application-managed guarantee and are disclosed separately.

First extraction is a bounded transaction: inspect the embedded manifest; check size/count/checked-byte arithmetic and available storage; acquire the profile extraction lock; stream into a fresh owned staging directory; validate paths and complete file digests; set the qualified permissions; then publish a ready profile atomically. Concurrent launches wait on that transaction and receive its actual result. A crash/disk-full event leaves the previous ready profile usable or a recoverable first-launch error. The app may retry/repair from its embedded payload offline; it cannot mark an incomplete directory ready or require a network download to recover standard functions.

Invoke helpers by absolute verified paths and give them an explicit working directory. The per-target profile must resolve private dependencies from its admitted component directories and OS libraries, not from a user's media folder or uncontrolled ambient PATH. Do not assume the parent's DLL loader configuration is inherited by FFmpeg; qualify the child's actual imported/dynamically loaded dependencies. Missing GPU driver components must still leave the ordinary CPU profile launchable. [Windows loader behavior](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-security)

`components verify` reports profile integrity and missing/corrupt files. `components repair` produces a staged replacement from the embedded or explicitly imported verified payload. It waits for affected active references to drain, or offers explicit cancellation, instead of replacing files underneath accepted jobs. Other independent profiles may remain usable. Component cleanup removes only unreferenced manifest-owned contents; corrupt/in-use state is not erased to make diagnostics green.

The initial protected data-store baselines are qualified local NTFS on Windows, ext4 on the selected Linux baseline and APFS on macOS, with the required ownership/permissions and filesystem operations actually tested. Other filesystems can be added by the same capability check. An exFAT/FAT32 media source is not prohibited, but a location lacking user-only credential protection is not silently admitted as the credential/store root. Explain the specific failed requirement and allow an explicit secure data root. Do not claim that “portable” means secure writable state on every filesystem. [Windows filesystem capabilities](https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison)

**D6-J01 — Portable launch/repair journey (WP02):** on a clean supported system, one EXE performs its first offline launch in a path containing spaces and Unicode, extracts exactly the declared standard profile, runs the shared CPU quickstart, and records every app-managed file location. Repeat with concurrent launches, disk full during extraction, a corrupt helper, an unwritable root and a root without required permissions. No test succeeds by finding a development FFmpeg on PATH. After repair, existing projects and accepted-job records remain intact.

#### Windows shell and MCP process contract

The single Windows EXE remains a GUI-subsystem application. In PowerShell 7, a direct GUI application at the end of an interactive pipeline can be started without the shell waiting for its result. Console attachment does not change that parent-shell decision. Therefore human instructions and automated examples that need a completed CLI result use a downstream pipeline stage: `ConvertFrom-Json` for JSON output and `Out-Host` for human output. Preserve inherited valid standard handles; attach to an available parent console only when needed for interactive CLI output, and keep UTF-8 stdout/stderr behavior correct under redirection. A no-argument GUI launch may return immediately. [PowerShell native-command waiting logic](https://github.com/PowerShell/PowerShell/blob/master/src/System.Management.Automation/engine/NativeCommandProcessor.cs) · [Microsoft AttachConsole contract](https://learn.microsoft.com/en-us/windows/console/attachconsole)

These representative PowerShell 7 examples wait for the frontend result and capture its native exit status immediately after that pipeline completes; the command reference uses this same convention:

```powershell
function Write-NwSetupLog {
    param(
        [ValidateSet('INFO', 'WARNING', 'ERROR', 'DEBUG')][string]$Level,
        [string]$Message
    )
    if ($Level -eq 'DEBUG' -and -not $NwSetupDebug) { return }
    [Console]::Error.WriteLine(('[{0:yyyy-MM-dd HH:mm:ss zzz}] [{1}] {2}' -f (Get-Date), $Level, $Message))
}
$NwSetupDebug = $false
Write-NwSetupLog INFO 'Starting the documented Numera Wiz command sequence.'
$Exe = '.\numera-wiz.exe'

$Capabilities = & $Exe capabilities inspect --json | ConvertFrom-Json
$CommandExit = $LASTEXITCODE
if ($CommandExit -ne 0) { Write-NwSetupLog ERROR "Capability inspection failed with exit code $CommandExit."; throw "Capability inspection failed with exit code $CommandExit." }
$Capabilities

& $Exe jobs list | Out-Host
$CommandExit = $LASTEXITCODE
if ($CommandExit -ne 0) { Write-NwSetupLog ERROR "Job listing failed with exit code $CommandExit."; throw "Job listing failed with exit code $CommandExit." }
```

For `jobs submit ... --wait --json`, use the same `ConvertFrom-Json` pipeline and exit-status check: `--wait` waits for the declared job outcome through the service API. Wait for the CLI frontend, not its detached long-lived worker descendants. Do not use a generic `Start-Process -Wait` process-tree wait as job coordination. An accepted job or another client's valid lease may keep the worker alive after the frontend returns; it must not retain the frontend's standard handles or prevent its output pipe from reaching EOF.

MCP clients start `numera-wiz.exe bridge` directly with explicitly owned stdin/stdout/stderr pipes and a process handle. They consume MCP framing themselves and wait for the bridge process according to the client lifecycle; they must not insert `ConvertFrom-Json`, `Out-Host` or another human display stage into the MCP stream. The bridge preserves clean protocol stdout, sends diagnostic logs to the separate stderr/log sink, and cannot lend its pipe handles to the detached worker. No additional Windows frontend executable is required.

### GitHub CI and owner-machine qualification

#### Build/test placement

All application, WASM, FFmpeg, dependency/profile and harness builds occur in GitHub Actions. All ordinary unit, integration, property, fuzz, media, browser/native UI, packaging, security and performance-regression tests run there. Local work is source inspection/editing and lightweight document/schema review where needed; normal developer instructions must not tell the owner to run `cargo test`, `cargo build`, local FFmpeg feature tests or model builds.

The local exception is actual GPU qualification on the owner's Windows 11 Enterprise / RTX 4070 Ti 12 GB machine using the downloaded, verified CI candidate and prebuilt Rust harness. Include a minimal desktop-launch/CPU baseline in that same qualification package where it establishes the real Windows 11 environment. It invokes no Cargo/compiler and needs no source checkout. Do not enroll the personal machine as an unrestricted public-PR runner or execute arbitrary PR scripts there.

Use explicit hosted runner/target labels, initially `ubuntu-24.04`, `windows-2025`, `macos-15-intel` and `macos-15`, plus the WASM target. The two macOS labels have different architectures. Hosted Windows Server is not a real Windows 11 desktop qualification. Verify current runner support when implementing rather than substituting `*-latest` without noticing an OS/architecture change. [GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)

#### Required suites and assertions

| Suite | Concrete cases and independent assertions |
|---|---|
| Domain/unit | Checked rational math, rounding, time domains, IDs, schema defaults/ranges, stream selectors, effect dependencies, graph cycle checks and deterministic serialization. |
| Edit/property | Random valid edit sequences preserve source coverage, ordering, lock/sync-lock and link invariants; inverse/undo restores data; overflow/invalid ranges fail atomically. |
| Planner | Copy eligibility, explicit maps, no lost requested edits, objective/rate-control compatibility, graph topology, sidecars, hardware transitions and stale-plan behavior. Golden argv is supplemented by actual output checks. |
| Media correctness | CPU fixtures exercise every required workflow and admitted effect domain; ffprobe/decoded frame/sample oracles verify timing, dimensions, layouts, metadata, alpha/color, boundaries and tails. |
| Cross-feature composition | Cuts + speed + overlay + multiple audio + captions + splits; GIF/thumbnail after edits; nested/multicam + transitions; loudness after final mix; output-mode changes with preserved intent. |
| Format matrix | Long-GOP/open-GOP, CFR/VFR, interlaced/telecined/mixed cadence, nonzero/negative starts, delayed/missing audio, 30000/1001, odd/subsampled crop geometry, SAR/rotation, 8/10/float domains where admitted, HDR/SDR, alpha, multichannel, attached pictures, fonts and bitmap/text subtitles. |
| Analysis | Silence/black endpoints and EOF, duplicate confirmation versus sparse-data heuristics, explicit metric input roles, controlled dropped/duplicated frame maps, permitted differing bit depths, partial EOF, missing/incomplete probe/result evidence and unknown results. |
| CPU policy | No media GPU device initialization; unavailable vendor media driver/DLL does not prevent startup; every core workflow executes on the selected software path. GUI presentation may initialize the selected graphics backend; headless modes initialize neither graphics nor audio devices. |
| Native preview | NUT header/PTS/payload oracle, negative/nonzero source starts, audio offset in both directions, preroll and delay compensation; prove defined nonnegative transport PTS and exact project-clock reconstruction. Exercise arbitrary read chunking, truncation/oversize/checksum rejection, unequal EOF, full queues and audio stall. A slow virtual clock bounds a fast producer's future-frame advancement; pause/seek/stale generation never drains the whole source. Delayed live-command replies cannot acknowledge a new generation; measure command acceptance separately from audible response. |
| Browser preview | The declared OS/browser matrix and frozen version manifest; real Safari evidence kept distinct from WebKit automation; authorized fetch/MSE append/evict, actual negotiated playback, seeking/reload, origin/epoch changes, hidden-tab cleanup, autoplay denial, canvas texture/overlays, bounded queues and A/V clock. |
| UI/shortcuts | Shared workspace/panel hierarchy, drag/dock/resizing, timeline gestures, inspector state, text/IME focus, effective FilmCraft Default bindings, conflict/remap persistence and reserved-key alternatives. |
| Text/graphics precision | Actual FFmpeg output for literal percent-expansion syntax, braces, backslashes/colons, Persian/Arabic bidi/shaping, fallback/missing glyphs and ASS composition-size changes. Verify HDR base precision through separate vector coverage/layers, premultiplied-alpha edges and zero-strength/map-plane variable-blur behavior. |
| Interface parity | Equivalent native/browser/CLI/MCP requests have the same normalized semantics, revision effects, availability, errors, plan and output assertions. Exercise both MCP transports. |
| Protocol | Both supported eras: modern discover/per-request metadata and legacy initialize/negotiation; framing/paging, clean stdio/EOF, version-specific HTTP headers/session behavior, malformed data, cancellation/disconnect, resource auth, long jobs and exact errors. |
| Idempotency/concurrency | Fresh authenticated replay-scope bootstrap without a prior scope; bounded repeated issuance; retry after lost response, deleted target, advanced revision and version/default update; key/content conflict; replay expiry/rotation/clock rollback/garbage collection without replay. Volatile control streams keep bounded acknowledgments instead of per-event durable receipts; stale generations fail, final edits commit once and revisions refer to the correct resource. |
| Recovery | Inject crash before/after edit commit, checkpoint, save rename, verification, publication fence and per-file publish. Reconcile actual outputs; never signal reused PIDs or lose acknowledged state. |
| Project ownership | Two data roots open the same UUID/path; copied UUID and Save As; aliases, external changes, stale lease and read-only fallback. Different UUIDs/roots race for the same destination: shared publication claims serialize final checks and atomic no-replace prevents unintended overwrite. Save As cannot replace another live writable project by acquiring only a new UUID. |
| Path/process security | Traversal, symlinks/junctions, hard links, source/output alias, Unicode/quotes/metacharacters, reserved option aliases, filter-file opens, concat protocol escapes and unintended network access. |
| HTTP/browser security | Missing/wrong/expired token, fragment reuse, exact Host/Origin, null origin, rebinding attempts, wrong port/session, redirect leakage, CSP/XSS and scoped file grants. Stdio/HTTP effective principals stay inside the selected access profile; an agent cannot grant itself broader roots or convert forwarded paths into owner authority. |
| Archive/component security | Zip/tar traversal, decompression bomb, count/size overflow, duplicate/case collisions, malicious symlinks, corrupt signatures/hashes, interrupted extraction and hostile optional packs. |
| Resource/soak | Long preview, giant bin/timeline, slow client, log flood, disk full, constrained-memory reverse, huge metadata, bounded model/IR, scheduler fairness, cancellation and process cleanup. Slow active probe/plan/download responses retain the worker beyond idle grace; disconnected or timed-out requests release their leases and bounded streams do not keep it alive forever. |
| Packaging | Offline first launch, helper imports, portable roots with spaces/non-ASCII, unwritable root, no compiler/runtime installed, concurrent extraction and rollback. The packaged GUI-subsystem EXE, invoked from PowerShell 7 with downstream JSON/human stages, waits for a deliberately delayed frontend result, preserves UTF-8 and its exact success/failure exit status, then reaches pipe EOF while an authorized detached worker/job may remain alive. Qualify cmd/process-spawn behavior and direct MCP client-owned pipes separately. |
| Dependency/security | RustSec audit, cargo-deny license/source/dependency checks, native dependency/CVE provenance, reviewed unsafe, secrets scan, workflow permission/pinning checks and SBOM/source consistency. |
| Fuzz/sanitizers | Request/project/sidecar parsers, NUT and packet framing, timeline/edit sequences, path/archive handling and protocol states. Exercise supported sanitizers/native tests on appropriate CI jobs. |
| Documentation | Every documented CLI example parses and exercises the intended fixture; schemas/help/manuals agree; human/agent setup and nondefault port work on packaged artifacts. |

Generate parameterized cases from the registry, but require independent expected behavior. A test that repeats the compiler's own calculation is not a sufficient oracle. Use synthetic/licensed small fixtures for frequent tests and larger pinned corpora for scheduled/release runs. Do not mutate golden expectations to make a failure disappear. Preserve test seeds, output assertions, failure artifacts and reproduction inputs with bounded retention.

Run fast compile/lint/unit/property and critical CPU/parity/security checks on PRs; affected platform/media/UI suites on meaningful changes; wider format/soak/fuzz matrices nightly; and the complete required distribution/CPU matrix on a release candidate. All suites exist in CI even if not every expensive suite runs for a documentation-only change. Required release checks cannot be silently skipped. A skipped hardware test is not a passed test.

When building FFmpeg, include relevant upstream FATE coverage and record `fate-list`, actual sample availability, skips and thread/profile settings. A green result with missing expected tests is not closure. Never use `GEN=1` to silently replace reference outputs. Run security tools only for languages/features they actually support; do not claim Rust CodeQL coverage from an unrelated-language scan.

GitHub workflows use least permissions, pinned action commits, protected signing/publication environments, no secrets for untrusted fork code, bounded concurrency and artifact checksums/provenance. CI generates the Rust language report and dependency graph, prevents forbidden donor media crates in shipped/native/web closures, and checks that required capabilities remain in the packaged FFmpeg build.

#### CI graph and evidence completeness

Use a small set of reusable workflows with these responsibilities. The names are selected repository file names; implementation may split a job for platform constraints without losing its evidence contract.

| Gate ID / workflow | Trigger / consumed inputs | Required output and gate |
|---|---|---|
| D6-G01 / `ci.yml` | PR/push; source and locked dependency/schema inputs | Rust formatting/lint/compile/unit/property and affected CPU/parity/security suites; required job inventory and exact result records. |
| D6-G02 / `components.yml` | Locked profile change or explicit release-candidate dispatch | Platform-native FFmpeg/dependency build, relevant FATE evidence, import inspection, profile manifests and immutable component artifacts. Cache by complete source/toolchain/configuration digest. |
| D6-G03 / `platform-qualification.yml` | Application/component candidate from CI | Packaged CPU/media/native UI/browser/transport/portable fixtures on every required target, plus the declared browser matrix and provenance-linked result index. |
| D6-G04 / `extended-qualification.yml` | Scheduled and release-candidate | Wider formats, resource/soak/fuzz/sanitizer/security evidence under bounded time/retention; fixed reproduction inputs for failures. |
| D6-G05 / `candidate.yml` | Protected explicit candidate dispatch | Build/package/sign where credentials exist, assemble final distribution bytes and owner harness/fixture pack, execute required packaged suites, generate `candidate-manifest.json` and unresolved-gate report. |
| D6-G06 / `owner-evidence.yml` | Authorized import of a bounded report for a named candidate | Validate schema, candidate/harness/fixture/config/device-policy identities and case results; store reviewed owner evidence. Never execute files or shell supplied by the report. |
| D6-G07 / `release.yml` | Protected promotion of a qualified candidate | Join all required evidence, verify exact artifact/source/provenance digests and publish the previously qualified bytes. Missing required evidence blocks promotion. |

The candidate orchestrator has a build phase and an evidence-collection phase, not a dependency cycle: G01 and the matching G02 component outputs precede its final package construction; G03/G04 consume those already-created immutable bytes; the G05 completion record joins their hosted results. G06 then imports the external owner result for that candidate. G07 depends on the required G01-G06 evidence and publishes without a build step. A scheduled G04 run can exercise an earlier candidate, but closes a later candidate's gate only where the recorded relevant input digests match.

Release gating reads a generated `required-evidence.json`, not the visual greenness of a workflow page. Each entry identifies requirement/domain/capability and variant, platform/browser/backend profile, fixture/suite, relevant input digest and required outcome. Every result identifies those same keys, actual artifact/runner versions, pass/fail/skip/pending status, assertions and retained evidence location. No absent row, empty suite, missing FATE sample, cancelled job or unsupported target is coerced into a pass. Conditional PR selection can reduce work; a release candidate must close the full required join.

Do not multiply runs for every donor alias when the canonical behavior is shared. A common lowerer has shared tests; meaningful parameter/domain variants and distinct project/session actions retain their own assertions. Generated schema/parity checks establish exposure, while media/edit oracles establish behavior. Scope exclusions remain explicit and cannot remove an admitted required capability merely to make the matrix pass.

Candidate workflows may consume previously built components only when the complete input digest and required provenance match. Build artifacts used for promotion are immutable and digest-checked. Sign/notarize/staple/package before the tests that qualify those final bytes; if a later operation changes the distributed artifact, regenerate its identity and rerun affected package/runtime gates. [GitHub artifact identity](https://docs.github.com/en/actions/tutorials/store-and-share-data)

**D6-J06 — CI completeness journey (WP12):** construct a candidate with one missing required platform result, one skipped required case and one stale input digest; the release join rejects each. A documentation-only change may reuse identical qualified payloads, while a changed lowering/component/fixture input invalidates affected rows. Check that donor aliases do not inflate required runtime case counts without a semantic distinction.

#### Owner GPU bundle

Release-candidate CI produces `numera-wiz`, a prebuilt `numera-qualify` executable, signed/checksummed fixture/profile manifests and instructions. The harness detects and records OS build, CPU/RAM, GPU/VRAM, driver, executable/profile/harness/fixture hashes and command plans. It runs only predeclared cases, writes timestamped structured reports and preserves bounded useful failure artifacts.

The RTX matrix includes H.264/HEVC/AV1 paths actually supported by the device/profile; admitted 8/10-bit decode/encode; CPU-filter/GPU-encode and GPU-filter routes; resolution/cadence/color correctness; two concurrent demands within budget; strict-required rejection; authorized fallback; cancellation/device failure; and VRAM/resource observation. Compare selected outputs with CPU reference semantics and declared quality tolerances. An encoder name in a log, GPU utilization alone or a nonempty file does not prove the requested path or correct result.

Tie qualification to actual application payload, FFmpeg profile, harness, fixtures, policy/configuration and device/driver hashes. Commit/workflow IDs remain provenance. Reuse a report only when CI proves the relevant qualification inputs identical; an executable/lowering/fixture/driver change invalidates affected evidence. Do not force a pointless local GPU rerun for identical binaries after a documentation-only commit, and do not reuse an old report for merely similar artifacts.

If this environment has not been run, mark GPU qualification pending and withhold the corresponding verified/release claim. Continue completing hosted CPU/platform work. Never fabricate the owner's result or move their GPU requirement onto a generic hosted runner.

#### Owner GPU report import and exact promotion

The local owner still runs only the verified CI-built application/harness and predeclared fixtures on Windows 11 Enterprise / RTX 4070 Ti 12 GB. No local Cargo, FFmpeg build or arbitrary PR script is introduced. The harness produces `owner-qualification.json` plus bounded evidence attachments and a hash manifest. Include candidate ID; exact application, profile, harness and fixture digests; tested settings; OS/device/driver observations; case results and reasoned skips; timing/resource samples; output assertions; and a redacted failure summary. A result is accepted only for the candidate and policy it actually exercised.

Provide an explicit return path: the owner imports the report/attachments as the named candidate's evidence artifact through a protected workflow or maintainer-reviewed repository evidence change. The validation job treats it as untrusted data, checks size/path/schema/digest limits and never executes supplied content. An owner-confirmed report is evidence supplied by the owner; a JSON signature alone is not proof of physical GPU execution. Do not require the personal PC to become a general CI runner.

The report must distinguish supported-and-passed, supported-but-failed, unavailable-on-this-device and not-run. The required RTX routes cannot be satisfied by a skip. Non-RTX platform acceleration can remain a clearly unqualified runtime capability until separately evidenced, while ordinary hosted CPU/platform work continues. The full requested stable release does not claim the owner qualification has passed before that report exists; an explicit development/candidate artifact may remain available with accurate pending status.

`release.yml` promotes the same final candidate artifacts. It does not rebuild them from the same commit and assume equivalence. Qualification reuse after documentation changes compares the actual relevant artifact/input digests; a changed driver or graph/compiler/fixture input invalidates the affected evidence. Repository/workflow attestation establishes build origin and byte identity, independently of test results. Retain the external attestation bundle needed for documented offline verification without making the GitHub CLI a normal app runtime dependency. [Attestation verification](https://cli.github.com/manual/gh_attestation_verify)

**D6-J07 — Owner evidence/promotion journey (WP12):** the owner runs the matching bundle; CI accepts its bounded report only for matching candidate inputs and reviews its status honestly. A tampered digest, renamed unrelated report, malformed attachment or required GPU skip cannot close the gate. Promotion preserves the exact final artifact bytes and never executes report-supplied commands.

### Repository manuals and release completion

#### Required documentation files

| Repository path | Required content |
|---|---|
| `README.md` | Product purpose, actual support/feature status, download/checksum/license, offline portable launch, browser-local engine, human and AI-agent quick starts, CPU/GPU choices and links. |
| `docs/install/windows.md`, `linux.md`, `macos.md` | Platform artifact, permissions, portable root, launch/CLI path, verification, update/rollback and honest signing/support limits; Windows examples distinguish asynchronous GUI launch, synchronous PowerShell CLI pipelines and direct MCP-owned process pipes. |
| `docs/agents/setup.md`, `docs/agents/compatibility.md` | Stdio and HTTP MCP connection, token/grant setup, default/custom port, capability discovery, plan→submit→watch→inspect→cancel workflow and error handling. |
| `docs/cli/reference.md`, `docs/cli/cookbook.md` | Every command/argument/default/unit/error/exit code; concrete FFmWiz utility and editing/batch recipes. |
| `docs/mcp/reference.md`, `docs/http-api.md` | Negotiation/tools/resources, paging, schemas, auth, lifecycle, job cancellation, idempotency, events and tested clients. |
| `docs/gui/guide.md`, `docs/gui/shortcuts.md` | FilmCraft-derived panels/workspaces, editing and utility journeys, shortcuts/focus, effect controls, recovery, command inspector and accessibility. |
| `docs/browser.md` | Same GUI with local engine, pairing/file grants/uploads, required OS/browser matrix, frozen tested versions and real Safari evidence, playback codecs, storage/backpressure and reconnect. |
| `docs/media/ffmpeg-plans.md` | Planner objectives, maps, formats, time/color/audio policy, inspected final commands, analysis-dependent stages and expert options. |
| `docs/media/effects.md`, `transitions.md`, `audio.md` | Generated native semantics/units, practical recipes, animation/latency/tail/resource limits, CPU/GPU routes and truthful donor differences. |
| `docs/media/capabilities.md` | Runtime/build/device states, missing components, qualified format/parameter domains and exact unsupported features. |
| `docs/project-format.md`, `docs/interchange.md` | Versioning, IDs/time, unknown-data preservation, journal/checkpoint/recovery, supported exchange subsets and conversion reports. |
| `docs/performance.md`, `docs/gpu-qualification.md` | Budgets, benchmark methodology/results, prebuilt owner harness, hashes and qualification reuse/invalidation. |
| `docs/security.md`, `SECURITY.md` | Actual trust boundaries, grants, process/secondary opens, auth, vulnerability reporting and supported versions. |
| `docs/development.md`, `docs/extensions.md` | Rules/Spec Kit, CI-first contribution, Rust workspace, full new-capability path, schemas/docs/tests and no local-build default. |
| `docs/release.md`, `docs/maintenance.md`, `THIRD_PARTY_NOTICES.md` | Component lock/build recipes, GPL/corresponding source, SBOM/provenance, signatures and release checklist. |
| `docs/research/`, `docs/vendor/ffmpeg/` | Pinned evidence, full manual archive/reading ledger, source comparisons, decisions and exact remaining qualifications. |
| `docs/requirements-traceability.csv`, `docs/development/continuation.md` | Original request-to-task/evidence links, truthful status and a resumable engineering state; no invented completed runs. |
| `benchmarks/workloads.json`, `required-evidence.json` | Frozen measurement inputs and the complete relational release evidence contract generated from the registry and supported platform matrix. |
| `docs/android-plan.md` | Later platform work, reusable boundaries and explicit current non-support. |

Do not satisfy this list with empty files, headings or links to one vague README. Human setup must lead from download to a verified small output. Agent setup must lead from connection to capability discovery, a dry plan, job submission and machine-readable result. Every example names actual implemented options and is checked in CI against the packaged artifact.

#### Required end-to-end release journeys

1. Open the portable app offline, import media, inspect it, apply a simple edit, inspect final commands, export on CPU and verify the result.
2. Open the same project in native and browser GUI; make an edit in one, observe coherent revision/state in the other, close/reconnect without losing accepted work.
3. Run a configuration-derived FFmWiz job through CLI, then the same semantic request through stdio and HTTP MCP; compare plans and outputs.
4. Complete a real multitrack edit with cuts, linked audio, transitions, effects, captions, graphics, mix, nesting/multicam where selected, save/reopen and export.
5. Perform batch/remux/extract/replace/metadata/chapter/cover tasks without requiring a timeline project, preserving requested streams and partial-failure results.
6. Interrupt a long job, restart/recover, retry safely, and prove no partial output is represented as successful or source file overwritten.
7. Run the qualified owner GPU plan and strict/fallback cases on the matching CI bundle; retain the actual report.
8. Install/setup as an AI agent using README alone, discover an effect/command schema, plan, execute, monitor, cancel/retry and read an authorized artifact.

The initial release is complete only when every fixed requirement, FFmWiz family, admitted donor functional row, command-domain journey and required platform gate is accounted for; all completed capabilities have interface parity; packages and full manuals exist; source/license/provenance artifacts match; resource claims have measurements; and pending external qualifications are named explicitly. A count of source IDs, read manual lines or test names is not a completion percentage.

The implementation handoff must report what shipped, what changed from donor semantics, exact unsupported/optional capabilities, tested platforms/browsers/devices, artifact links/hashes and any remaining blocker. Continue fixing ordinary implementation/CI failures autonomously. Ask for user input only where a real unresolved product choice or unavailable external credential/hardware action requires it, after making the proposed result concrete and reviewable.


