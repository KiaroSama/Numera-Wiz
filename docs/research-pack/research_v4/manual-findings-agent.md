# V4 FFmpeg full-body reading completion — assigned manuals and later filter sections

**Reviewer:** v4_manuals_finish  
**Source commit:** `946fcce07b6dcd0331c8cc609192aeff5e1924f8`  
**Evidence:** `manual-reading-events-agent.json`  
**Method:** Full source bodies were displayed and read in bounded, untruncated tool outputs. Indexing and searches were not counted as reading. An initial truncated developer-manual output was discarded and the entire file was reread in two bounded ranges.

## Completion and limits

The original assignment is complete: **17,141 source lines**, consisting of **5,174 lines in eight non-filter manuals** and **11,967 lines in all 334 assigned unread/partially read filter sections at or above line 18000**. There are no unfinished ranges in this assignment. The original v3 ledger was not changed.

The eight complete manuals are community, developer, FAQ, FATE, general contents, Git how-to, mailing-list FAQ, and platform. All reading events preserve actual filename and inclusive line ranges. This establishes source reading, not implementation, command execution, portability, security, visual quality, or performance proof. No application builds, FFmpeg execution, media processing, or tests were run. Referenced Doxygen pages, third-party papers, and linked external websites are not automatically included in this completion claim.

Sources below refer to the exact pinned upstream path, with local filenames obtained by replacing `doc/` with `doc_`. Base:
https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/

## Mandatory changes to the construction mandate

### 1. Low-memory requirements must constrain filter-specific retained state

The memory policy must describe FFmpeg worker allocations as well as Rust application allocations. A bounded UI cache does not bound a filter's own retained frames.

- **Thumbnail selection:** `thumbnail` retains the full frame batch, whose default is 100. Its manual explicitly warns about higher memory use for larger batches. The upstream illustrative recipe places `scale` *after* `thumbnail`; copying that recipe into a timeline service preserves full-resolution batch memory. For a 3840×2160 8-bit 4:2:0 frame, even the simple tightly packed data estimate is 12,441,600 bytes; 100 such frames are approximately 1.159 GiB before other allocations. This is an illustrative lower-bound calculation, not measured process RSS. A low-memory preview plan must use targeted seeks, a bounded batch, or scaling before representative-frame selection. Scaling first can change which frame wins, so record that as an approximate preview decision.
- **Temporal filters:** `random`, `tmedian`, `tmidequalizer`, `tmix`, `tiltandshift`, and stabilization need individual horizon/resource entries. Do not assume every filter streams with a single input frame.
- **OpenCL stabilization:** `deshake_opencl.smooth_window_multiplier` controls a buffered window proportional to input frame rate; the manual explicitly links larger values to latency and memory.
- **Vulkan denoise:** `nlmeans_vulkan.t` raises parallelism and VRAM use but need not increase speed; the best value depends on hardware and input.
- **Generated inputs:** source filters commonly generate forever unless bounded. Job plans and CI fixtures must provide explicit finite duration/frame bounds.
- **Hald CLUT:** level N creates N³×N³ pixels, so pixel count grows as N⁶. Reject impractical dimensions before worker launch; do not infer low cost from a small integer level.

Evidence: `doc/filters.texi` 23696–23729, 24100–24199, 27998 onward within 27664–28320, 29421 onward within 28971–29620, and generated-source chapters 29621–31098.

**Required fields:** estimate kind, known retained-frame or sample horizon, scratch space, frame format/bit depth, GPU memory estimate, unknown-cost marker, maximum resource policy, cancellation behavior, and measurement evidence. Estimates must remain estimates until measured.

### 2. GPU variants are semantically different implementations

Do not implement acceleration by adding a backend suffix to an arbitrary CPU filter name. Keep a canonical operation with backend-specific eligibility and parameter mapping.

| Example | Pinned documentation difference | Required planner behavior |
|---|---|---|
| `tonemap_opencl` | Desaturation algorithm differs from CPU tone-map; default desaturation is 0.5; output formats limited to documented p010/nv12 choices. | Compare rendered meaning and color metadata under declared tolerance; no bit-identical fallback claim. |
| `tonemap_vaapi` | Accepts HDR10 input; documented HDR-to-SDR and HDR-to-HDR behavior and output metadata differ. | Reject unsupported HDR inputs or choose a qualified fallback; never silently accept HLG merely because a different backend supports it. |
| `blend_vulkan` | Documented blend-mode set is normal/multiply. | Filter catalog must expose the actual backend mode intersection, with CPU fallback for additional modes when qualified. |
| GPU hstack/vstack/xstack | VAAPI/QSV variants can rescale differing input sizes; CPU vstack requires common width and pixel format. | Normalize intended canvas geometry explicitly so backend selection does not change composition. |
| `overlay_opencl` / `overlay_vulkan` | OpenCL discusses compatible color-plane memory layouts with optional alpha; Vulkan requires the same pixel format for every input. | Validate device, frame format, alpha interpretation, layout and conversion nodes per backend. |
| `nlmeans_vulkan` | More formats and alpha support than CPU/OpenCL, different strength range and per-component controls. | Define fallback policy per option and representation. |
| `mestimate_d3d12` | Requires D3D12 hardware frames and only 8/16 macroblock sizes. | Treat device/format/block size as capability prerequisites. |

Evidence: `doc/filters.texi` 18009–18650; OpenCL chapter 27664–28700; VAAPI chapter 28701–29077; Vulkan/QSV chapters 29150–29620.

Explicit uploads/downloads and supported formats matter. Hardware filters cannot simply operate on normal-memory frames. A supported filter name does not establish a usable device, compatible surfaces, favorable transfer cost, or faster execution.

### 3. Keyframes and live updates require per-option evaluation phases

Some filter options are evaluated at initialization, some on each frame, and some accept runtime commands. These are separate capabilities.

- `rotate.angle` is evaluated per frame, but output width/height are evaluated once at configuration. Only angle is documented as a runtime command.
- `normalize` excludes smoothing from its commands.
- `nnedi` excludes weights from its commands.
- `tmedian` excludes radius; `xmedian` excludes input count.
- `scroll` exposes horizontal/vertical speed commands, not every initialization option.
- `avectorscope` excludes size/rate from commands.
- `qrencodesrc` width expressions are initialization-time expressions, unlike per-frame QR overlay geometry.

Evidence: `doc/filters.texi` 18651–19292, 21077–21297, 21817–21981, 24100–24199, 26415–26831, 30261–30895, 30896–31527.

The feature registry needs `evaluation_phase`, `runtime_command_supported`, `requires_graph_rebuild`, valid expression symbols, and timeline segment behavior. An agent must not promise smooth keyframe animation merely because a numeric option exists.

### 4. Quality metrics need distinct contracts and honest labels

- PSNR/SSIM/VIF documentation requires aligned comparable videos with matching resolution and pixel format and assumes corresponding frames. Matching duration alone is insufficient.
- `xpsnr` differs: input one is the reference, input two is the test signal; differing bit depths are allowed while other pixel-format properties must agree. Its guidance favors the minimum of color-plane averages. Do not reuse PSNR input roles or a generic same-bit-depth validator blindly.
- `vmafmotion` is a **component motion metric**, not a complete VMAF quality score.
- `siti` explicitly implements the superseded P.910 November 2021 formulation; do not label its output as current P.910 compliance.
- Some metric stats files can be routed to stdout. The bridge must keep worker stats off the MCP protocol stdout and deliver them through owned files or captured pipes.
- `showvolume` peak/RMS and `showwavespic` average/peak are different measures. A waveform rendering is not a loudness compliance measurement.

Evidence: `doc/filters.texi` 20149–20897, 22752–22772, 22901–23550, 25074–25674, 25720–25738, 26440–26518, 33321–33944.

Metric receipts must preserve algorithm/version, input roles, selected streams, temporal alignment, color normalization, window/subsampling, missing frames, and infinite/undefined numeric handling.

### 5. Inspection has incomplete/unknown states

The FAQ explains that VOB and other formats without a complete global stream inventory can contain streams beginning after the initial probe. A fast initial probe cannot prove a subtitle/audio stream is absent. The FAQ discusses `probesize` and `analyzeduration`, but its illustrative default sizes must not be frozen as a universal property of every configured build.

`AVStream.r_frame_rate` explicitly is not average FPS; it can be the rate capable of representing the observed timestamps (e.g. 150 for mixed 25/30 content). Keep nominal rate, average rate, time base and actual frame timestamps separate. `vfrdet` observes timestamp-delta variation; it does not eliminate the need for precise frame indexing.

Evidence: `doc/faq.texi` 440–456, 665–672; `doc/filters.texi` vfrdet within 25074–25674.

Required outputs: probe extent, configured limits, completeness/confidence status, unknown fields, follow-up bounded scan action and cancellation. An absent initial field must not be serialized as a confident false.

### 6. Subtitle fidelity requires a declared font/shaping build profile

The `subtitles` filter requires libass, libavcodec and libavformat for conversion. Complex shaping, relevant to Persian/Arabic as well as other scripts, requires HarfBuzz support in libass; `wrap_unicode` requires a sufficiently recent libass and libunibreak. `original_size` matters when ASS composition aspect ratio changes.

Evidence: `doc/filters.texi` 23349–23447.

Qualification fixtures must include Persian/Arabic joining, mixed RTL/LTR, combining marks, fallback glyphs, line breaking, styled ASS and aspect-ratio changes. Package owned fonts with notices, stable asset hashes and explicit resolution policy. The same project must not render with an unnoticed host-font substitution across native preview, browser preview and export.

### 7. Geometry and timing deserve semantic validation

- `scale` preserves display aspect by adjusting SAR while `crop` preserves SAR and changes DAR, as explained by the FAQ. Padding anamorphic video to a display aspect must include SAR.
- `transpose` numeric shortcuts are deprecated in favor of named directions and a separate passthrough mode.
- `zoompan` defaults include 90 output frames per input image, 25 FPS and hd720 size. Never inject it into ordinary video with those defaults accidentally.
- `separatefields` doubles frame count/rate while halving height; `weave` reverses that, and `doubleweave` has distinct rate behavior.
- `tile` examples require output FPS passthrough to avoid automatic duplicates.
- Sources round durations up to whole frame duration. A requested decimal duration is not always an exact emitted media duration.

Evidence: `doc/faq.texi` 472–495; `doc/filters.texi` 19528–19853, 21817–21981, 23551–24199, 24350–24428, 25675–26275, 26415–26831, 30261–30895.

Use rational time/frame counts, explicit output policy and endpoint conventions. Treat effect duration, source duration and output duration as distinct.

### 8. Analysis filters must not alter the main stream accidentally

`aphasemeter` rematrixes non-stereo audio to stereo and enables a video output by default. Analysis needs an isolated branch; do not splice it into an otherwise unchanged multichannel export and call it harmless inspection. Its phase range and mono/out-of-phase event detection are different outputs.

`showinfo` enables plane checksums by default. Do not leave expensive per-frame diagnostics active in normal preview merely to obtain timestamps.

Evidence: `doc/filters.texi` 22255–22900 and 31292 onward within 30896–31527.

### 9. Multi-pass and file-based operations need owned dependencies

`vidstabdetect` writes a transform file for `vidstabtransform`. Transform results must be tied to exact input identity, stream selection, trim interval, frame order and any preprocessing. Reusing a transform after a timeline edit without checking identity is invalid. Use owned scratch paths instead of shared default `transforms.trf` and account for debug `global_motions.trf` writes.

External weights/models, OCIO config/context, .asv selective-color files, masks, fonts and LUT assets are part of reproducibility and permissions. NNEDI cannot operate without its weights file; OCIO may otherwise depend on an environment variable. Avoid inheriting accidental host configuration into a supposedly reproducible render.

Evidence: `doc/filters.texi` 18651–19292, 21817–21981, 25284–25527.

### 10. FFmpeg support alone is not enough to make an input safe

The general manual explicitly declines autodetection of VapourSynth scripts for security reasons. AviSynth/VapourSynth, frei0r/LADSPA/LV2 plugins, custom OpenCL programs and similar facilities can load or execute code. A shell-free argv builder is necessary but does not neutralize those features.

For the ordinary catalog, prefer shipped reviewed declarative effects and FFmpeg-native parameterization. Imported projects and MCP payloads cannot activate arbitrary native plugins, model downloads, script sources, or GPU kernels solely because FFmpeg supports them. Provide typed trusted-asset declarations and an explicitly separate advanced capability where needed.

Evidence: `doc/general_contents.texi` external-library chapter; `doc/filters.texi` 27664–28320, 28321–28970, 30261–30895, 30896–31527.

This is a concrete boundary from the read manuals; it is not a blanket prohibition on supported effects.

### 11. Capture behavior must be represented accurately

`ddagrab` produces D3D11 frames; requested frame rate is a maximum/polling policy, and it does not buffer capture in the background. `gfxcapture` follows compositor timing, caps at max_framerate and ends when the selected window/display disappears. A fixed-FPS export requires an explicit FPS policy.

Window regex selection chooses the first match in OS-returned order. A UI should show the selected capture source and retain a validated handle rather than treating an ambiguous regex as a stable target. Capture is a separate permission-bearing operation from reading an existing project.

Evidence: `doc/filters.texi` 29868–30130.

### 12. Upstream manuals contain historical material and contradictions

The platform manual explicitly excludes ARM64EC. Native Windows ARM64 and ARM64EC must not be conflated. It also contains older toolchain recipes (VS2013-era tested configurations, obsolete Cygwin flags); these are archived text, not sufficient proof of a modern build recipe.

The general tables contain historical omissions or inconsistent descriptions, for example legacy-only device lists compared with the newer capture filters, and rows using internal/external markers inconsistently. Feature availability must be checked against the pinned source/configuration and actual packaged binary. A generated product support matrix must not copy the general table as authoritative installed capability.

Evidence: `doc/platform.texi` 95–349; all of `doc/general_contents.texi`.

Record source contradictions as research issues. Resolve at pinned source + qualified command level, without rewriting source evidence to make it look consistent.

### 13. FATE and CI evidence must describe what actually ran

Pinned FATE guidance supplies several useful controls:

- Capture `fate-list` for the actual configured artifact and compare expected test presence. Sample availability determines coverage; a green exit with missing expected tests is insufficient.
- On Windows, use the documented relative sample path constraints.
- `--assert-level=2` is appropriate for diagnostic builds, but distinguish those artifacts from release performance measurements.
- Vary decoder threading with recorded `THREADS`/`THREAD_TYPE`; preserve the actual randomized thread count from failure logs.
- `HWACCEL` defaults to none. Owner-local GPU tests remain separately identified; an emulated or wrapped `TARGET_EXEC` result is not native execution.
- `GEN=1` regenerates reference results. A repair agent must not use it to turn failing comparisons green without reviewed justification.
- FATE covers the upstream component; Numera's orchestration, policy, UI parity, bridge lifecycle and packaging require their own tests.
- Upstream publishing/reporting examples are not authorization to send messages or upload data.

Evidence: `doc/fate.texi` 1–300, especially 102–125 and 227–299; `doc/developer.texi` regression/coverage chapters.

The CI manifest should distinguish expected, executed, passed, failed, skipped and unavailable tests, with skip reasons. Avoid counting tests or lines as substitutes for branch/risk coverage.

### 14. Contributor guidance is not product scope or action authorization

The developer manual describes FFmpeg's C11 implementation/C99 public headers, SIMD runtime dispatch and checkasm tests. This confirms that “almost all first-party Numera code in Rust” cannot be represented as “the entire dependency/runtime graph is pure Rust.” FFmpeg and optional codecs remain native non-Rust dependencies.

Public library API/ABI guarantees apply within library major versions and to documented valid callers; they do not guarantee unchanged filter output, CLI behavior or performance. Preserve binary/source/configuration identity and requalify behavior on upgrades.

Community/Git/mailing-list instructions describe contributing to upstream, not mandatory changes to the user's global Git configuration, subscriptions, mail sending or upstream publication. Support bundles should have exact command/build context and reproducible evidence with privacy redaction, not undisclosed automatic reporting.

Evidence: `doc/developer.texi` 1–1184, `doc/community.texi` 1–182, `doc/git-howto.texi` 1–484, `doc/mailing-list-faq.texi` 1–389.

## Implementation artifacts this reading should change

1. Capability schema with backend-specific pixel/surface/color/audio/command-phase contracts.
2. Cost schema and preview planner covering retained frame/sample state, VRAM and finite generated-input bounds.
3. Probe-result schema with confidence/completeness and explicit deep-scan states.
4. Metric-job schema, ordered input roles and reproducible normalization.
5. Shaping/font dependency manifest and multilingual render fixtures.
6. Typed trusted-asset declarations and worker policy for scripts/plugins/kernels.
7. Multi-pass artifact identity and owned scratch paths.
8. CI evidence schema including expected test inventory, skips, FATE reference changes and owner GPU provenance.
9. Research issue log separating upstream text, source interpretation and qualified observed behavior.
10. Agent instructions preventing any statement of implementation/qualification completion based solely on this reading ledger.

## Additional completion: root batches 12–25

At the parent's follow-up request, all bodies in zero-based batches **12 through 25** of `manual-reading-batches-small-root.json` were then read. This adds **6,512 lines in 206 preserved split ranges**, recorded separately in `manual-reading-events-agent-extra.json`. These ranges are disjoint from the original assignment. Total actual reading recorded by this reviewer is **23,653 lines**. No extra-assignment ranges remain unfinished.

### A. An official example contains a demonstrably wrong fade expression

The `hue` examples include a generic fade-in expression equivalent to:

```text
min(0, max((t - START) / DURATION, 1))
```

For finite input and positive duration, the inner maximum is at least 1 and the outer minimum is therefore always 0. It cannot produce the described fade-in. A correctly bounded linear ramp is:

```text
clip((t - START) / DURATION, 0, 1)
```

or the equivalent `max(0,min((t-START)/DURATION,1))`. This is algebraic verification, not a claimed FFmpeg runtime test. Preserve the original archived manual; add a documented correction in the recipe corpus and a CI execution fixture. This demonstrates why “official docs read” must not substitute for recipe qualification.

Evidence: `doc/filters.texi` 16016–16061.

### B. CUDA VMAF documentation cannot be copied into a redistributable release build

The pinned `libvmaf_cuda` build example explicitly contains `--enable-nonfree`. The construction mandate must prohibit blindly using that recipe for public distributable GPL release artifacts. Determine the exact selected-source dependency/license condition; either qualify a redistributable configuration or omit that backend from the release profile and retain eligible CPU VMAF support. Merely saying “supports FFmpeg” does not resolve redistributability.

The CPU `libvmaf` contract has distorted input first, reference second, configurable model/features, pooling method, threads and subsampling. XPSNR uses the opposite semantic input order. Store roles rather than generic “input A/input B.”

Evidence: `doc/filters.texi` 17442–17541.

### C. Preserve frame correspondence through frame-rate conversion

The `fsync` filter is explicitly designed to reconstruct output-frame correspondence from a map produced by `-stats_mux_pre` with fields `{ptsi} {pts} {tb}`. It requires a map sorted by input PTS and can drop/duplicate reference frames to match the converted sequence before quality comparison.

The planner should retain an owned frame-map artifact when a conversion changes cadence and a metric job needs valid correspondence. Resetting PTS origins alone does not prove that frame n in one input is the matching content in the other. A stable Rust numeric sort/parse step should produce the actual sorted file; the documentation's illustrative `sort -n MAP_FILE` merely prints a sorted view unless its output is captured.

Evidence: `doc/filters.texi` 15019–15058.

### D. Text rendering needs separate literal and template semantics

Ordinary titles should default to literal UTF-8 text, using `expansion=none`, with owned text files where useful to reduce filter-escaping layers. A separate explicit template mode may enable metadata, PTS and time expansions.

- `text` and `textfile` are mutually exclusive.
- `textfile.reload` requires atomic updates to avoid partial reads.
- `fontfile` is mandatory without fontconfig.
- Drawtext compilation needs FreeType and HarfBuzz; default family/fallback needs fontconfig; RTL joining/bidi option needs FriBidi.
- Timecode rate is rounded to an integer; drop-frame timecode has explicit supported-rate limitations. Keep media rate, timeline time and display timecode distinct.
- Invalid `reinit` leaves old parameters in place. A command-delivery success cannot by itself mean the intended visual parameter update took effect.
- Wallclock `localtime`/`gmtime` expansions are distinct from frame-PTS expansions and undermine repeatable export unless explicitly selected.

Evidence: `doc/filters.texi` 12629–13235.

### E. Crop semantics need an explicit preview/export contract

`crop` width/height expressions run at configuration or on a command; x/y run per frame. Subsampled formats may round crop geometry unless `exact=1`. Invalid positions may be approximated to valid coordinates by the filter. `keep_aspect` can alter SAR to preserve original DAR.

The project model should define the user's exact intended rectangle and present any codec/chroma-alignment adjustment. A generic crop command must not silently convert a requested odd-pixel rectangle to a different one while the GUI shows the original. Runtime size changes must also be compatible with the downstream graph and encoder, not just with the crop filter's command parser.

`cropdetect` is an analysis recommendation; default reset behavior keeps the largest area encountered and initial frames may be skipped. Store analysis extent, assumptions and confidence; changing an analysis threshold is different from applying the result.

Evidence: `doc/filters.texi` 10794–11091.

### F. Several “same effect” names hide important scope limits

- `colorkey` operates on 8-bit RGB, `chromakey` is YUV, and `hsvkey` has another parameter domain. Avoid implicit HDR/high-bit-depth reduction for an effect without recording it.
- `grayworld` requires linear-light input. A metadata tag alone does not linearize pixels.
- `gradfun` explicitly is designed for playback and warns against use before lossy compression because compression can remove dither. Do not advertise it as a universal export improvement.
- `dctdnoiz` is expressly not designed for realtime. Expensive filters need an honest preview policy and optional lower-cost preview approximation.
- `framerate` is not designed for interlaced media; its documented path deinterlaces before interpolation and reinterlaces if required.
- `fieldmatch` reconstructs fields but does not remove duplicates; inverse telecine needs an additional cadence stage.
- `fieldorder` physically shifts pixels one line; it is not a metadata-only field-order setter.
- `frc_amf` doubles frame rate; it is not a general arbitrary-target-FPS replacement for `minterpolate`.
- D3D12 deinterlacing can use driver-defined algorithms and driver-dependent temporal reference queues. Preserve driver/device identity in quality/performance evidence.
- CoreImage can use software OpenGL and does not guarantee hardware execution; its documented filter wrapper supports zero/one-input effects and does not make all Apple transitions usable as intended.

Evidence: `doc/filters.texi` 9323–9488, 9973–10024, 10632–10735, 11332–11398, 11663–11761, 14035–14392, 14748–14859, 15235–15397.

### G. Asset identity must include database selection and environment resolution

Lensfun selects the best matching database entry from possibly incomplete make/model strings and reports its choice. Its database path otherwise depends on installation. Archive database identity and the actual selected camera/lens entry; preserve focal length/aperture/focus assumptions. Do not silently change correction when a host database update changes a fuzzy match.

Frei0r searches an environment-defined path and standard user/system plugin locations. Packaged execution should use a controlled plugin set if this capability is enabled; inherited search paths must not make the rendered project depend on arbitrary native host code.

Evidence: `doc/filters.texi` 14920–14986 and 16562–16699.

### H. Detection results are evidence with uncertainty, not facts without qualification

`idet` reports undetermined frames and distinguishes single-frame and history-based classifications. Its optional interlaced-flag analysis can clear/set reliance on those flags, so it must be isolated from an unchanged-export branch.

`colordetect` makes deductions from observed pixel ranges/alpha relationships; unavailable properties are ignored. A bounded sample that happens not to contain extreme values is not conclusive evidence for the opposite property.

`find_rect` only returns one instance when multiple copies exist. A detection-based masking feature must not claim all occurrences were found. `freezedetect` reports an event only after its threshold duration; event start time differs from event-report time.

Evidence: `doc/filters.texi` 9919–9945, 14454–14529, 14871–14899, 16213–16305.

### I. Read-time contradictions need a tracked resolution process

The CPU convolution manual describes an unset/zero reciprocal divisor as 1/sum of matrix elements, while the OpenCL wording states sum of elements. The exact implementations must resolve this before backend mapping or default generation. The fieldmatch narrative says decimate expects CFR, while decimate itself exposes a `mixed` VFR output option; these are different input/output claims and should be documented precisely rather than treated as equivalent blanket statements.

The metadata, recipe and test generators must retain provenance and record disagreement instead of picking whichever sentence makes the implementation easiest.

Evidence: `doc/filters.texi` 10527–10563, 11509–11552, 14035–14257, and convolution_opencl in 27664–28320.

### J. Useful diagnostics should remain bounded and purpose-driven

`graphmonitor` exposes queued frames, link time bases, formats, PTS deltas, counts and EOF status. It can help investigate stalls and hidden queue growth in CI diagnostic runs. It renders a video and therefore adds overhead; it is not a zero-cost always-on production monitor.

`bench`/ `abench` measure a selected graph interval using metadata and report times. They complement process-level startup/RSS/VRAM/energy measurements and do not replace them.

Evidence: `doc/filters.texi` 15286–15385 and bench section within 30896–31527.


