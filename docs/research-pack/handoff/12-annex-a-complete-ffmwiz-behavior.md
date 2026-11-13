> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## Annex A. Complete FFmWiz behavior

**Research snapshot:** 2026-10-21.  
**Donor:** [KiaroSama/FFmWiz](https://github.com/KiaroSama/FFmWiz).  
**Pinned commit:** `661a0d240ee29ca7cda496b635ae8c50e43d916b`.  
**Purpose:** an implementation-ready baseline appendix for the Numera Wiz mandate, covering the actual fifteen FFmWiz workflow families, the subordinate features hidden inside those workflows, every distributed configuration key, and the behavior that must change during the Rust rewrite.

### Evidence boundaries

The GitHub default head was checked and matched the commit above. The research mirrored 129 non-GUI source/configuration/license paths, approximately 2 MB, under `research_v2/sources/ffmwiz/`. The accompanying `ffmwiz-source-inventory.json` records source paths, SHA-256 hashes, 1,354 statically parsed function locations, forty configuration defaults and their literal access sites. This inventory is a reproducible index, not a statement that every function has been dynamically validated.

This appendix distinguishes:

- **Observed:** a behavior directly visible in pinned implementation or configuration.
- **Source-confirmed limitation:** a branch explicitly rejects, ignores, narrows or replaces a requested operation; no runtime reproduction is claimed.
- **Static risk:** implementation appears incomplete or incorrectly generalized, but runtime behavior was not tested.
- **Numera requirement:** the proposed replacement contract. It is deliberately more precise and consistent than the donor.

No donor application code, builds, media transformations or test suites were executed during this research. FFmpeg lowering recipes below are schematic plans; concrete executable options must be resolved against the project's pinned FFmpeg build and the actual media probe. The FFmpeg research companion supplies the versioned upstream manual and encoder/filter validation.

The donor menu has fifteen entries, but these are not fifteen independent processing engines. Several duplicate capabilities with different constraints. Numera should retain the user-visible tasks while consolidating their implementation into one typed operation registry and one planner. [S-entry]

| ID | Actual donor menu family | Proposed primary operation IDs |
|---|---|---|
| F01 | Interactive encode wizard | `media.encode`, `timeline.render` |
| F02 | Configuration-driven encode wizard | `config.import.ffmwiz`, then the same encode/render operations |
| F03 | Cut video without re-encoding | `media.cut` |
| F04 | Encode a folder | `batch.plan`, `batch.submit` using ordinary per-file operations |
| F05 | Add external audio/subtitle files to video | `streams.add` |
| F06 | Extract video/audio/subtitle streams | `streams.extract` |
| F07 | Media information and analysis report | `media.probe`, `media.report`, `media.compare` |
| F08 | Stream cleanup/remux | `streams.clean` |
| F09 | Hard subtitles | `subtitles.burn` |
| F10 | Video speed/reverse | `media.retime` |
| F11 | Audio cut/speed/reverse | `audio.edit` |
| F12 | Join media files | `media.join` |
| F13 | Metadata editor | `metadata.patch`, `chapters.*`, `cover.*`, `video-signaling.patch` |
| F14 | Capability cache tools | `capabilities.inspect`, `capabilities.refresh`, `capabilities.clear` |
| F15 | Track manager: remove/add/replace and loudness | `streams.replace`, `streams.remove`, `audio.normalize` |

Names in the final mandate may be harmonized with the global command registry. The capabilities, typed inputs, errors and acceptance scenarios must remain traceable even if a public command is renamed.

### Actual CLI and configuration contract

#### Do not describe the donor as an existing machine API

`FFmWiz.py:595–713` handles a small set of startup flags: `--refresh-ffmpeg-reference`/`--regen-ffmpeg-reference` and `--preview-colors`/`--preview-progress-colors`. Unknown arguments are reported and ignored. The main functionality is selected through terminal menus. There is no observed complete subcommand API, JSON job protocol, HTTP API or MCP server to port.

Mode 2 is also not headless automation. In `run_wizard`, nonempty configuration values skip associated prompts, but the unified graphical editor and final “start now” question remain interactive. Join, cut and audio-cut steps are not automatically skipped. Compositing is prompt-only and has no `video_composite` configuration key. Look/quick/volume/raw-option prompts have special gates when their configuration keys are absent. Numera must replace this with explicit deterministic requests, with optional interactive assistance as a client of the same API. [S-entry] [S-flow]

`parse_env_config` accepts blank lines, `#` comments, an optional `export ` prefix, `key=value` pairs and matching outer quotes. It ignores malformed lines without `=` and retains unknown keys. A legacy importer should report these conditions. New native configuration must use a versioned schema and reject unknown keys unless an explicit compatibility mode was requested. [S-env-parser]

#### All forty distributed configuration keys

The “legacy value” column is the literal shipped `config.env.example` value, not the recommended default for a new Numera project. Blank means a prompt in donor Mode 2. The new schema uses enums and typed values instead of overloaded `n`/`y` strings. [S-config] [S-config-video] [S-config-audio] [S-config-misc]

| Legacy key, line | Legacy value | Observed meaning and required Numera mapping |
|---|---|---|
| `input_path`, 71 | blank | Source file. Import as an asset reference; resolve relative paths against the configuration location, not an accidental process working directory. |
| `output_path`, 77 | blank | May mean a directory, full output filename or bare basename. Replace with `OutputTarget::{File,DirectoryTemplate}` and report the importer's interpretation. |
| `output_format`, 83 | `n` | Keep source extension/container. Map to `ContainerPolicy::Source`, with actual muxer compatibility checked. |
| `video_codec`, 94 | `H265` | H.265 alias by default; H.264, AV1, VP9, MPEG-4, copy and actual runtime encoder names are accepted. Config `n` means copy. Separate codec family, encoder implementation and copy strategy. |
| `use_gpu`, 99 | `y` | Prefer usable NVIDIA acceleration; it is not a promise that all stages run on GPU. Import as automatic preference, not `GpuRequired`. |
| `crop`, 105 | `n` | Enable crop; disabled ignores the four margins. Map to an optional crop node. |
| `crop_top`, 108 | `0` | Nonnegative pixels removed from the top. Validate against actual post-rotation geometry and chroma alignment. |
| `crop_left`, 109 | `0` | Nonnegative left removal. |
| `crop_right`, 110 | `0` | Nonnegative right removal. |
| `crop_bottom`, 111 | `0` | Nonnegative bottom removal. |
| `video_bitrate_kbps`, 116 | `n` | Keep detected source bitrate. This is not a universal perceptual quality target. Map to `RateControl::SourceBitrate` with known/estimated/unknown provenance. |
| `video_bitrate_mode`, 121 | `quality_vbr` | `quality_vbr` uses bitrate B, maxrate 2B, buffer 4B; `strict_size` uses B/B/2B. Preserve these as legacy profiles, not a promise of exact output bytes. |
| `resolution`, 128 | `n` | Source, named/plain closest-edge presets, forced width/height, aspect-preserving target box, or explicit stretch. Use a tagged geometry enum, never infer all forms as exact dimensions. |
| `fps`, 133 | `n` | Preserve source timing; donor numeric entry is integer-only. Numera must also accept rational rates such as `30000/1001` and explicit VFR/CFR policy. |
| `video_look`, 145 | `n` | Compact rotate/flip/gray/brightness/contrast/saturation/denoise/sharpen/blur/fade syntax; see F01. Import into an ordered effect list. |
| `video_quick`, 154 | `n` | GIF, boomerang, thumbnail, loop tokens; see F01. These are real baseline features, not future ideas. |
| `audio_volume`, 161 | `n` | Linear factor, percent or dB; accepted factor 0.01–10. Applied after loudness normalization in the donor chain. |
| `raw_ffmpeg_args`, 169 | `n` | Extra argv tokens with reserved-option and declared-arity validation. Preserve a controlled expert mechanism, never a shell string. |
| `raw_ffmpeg_valued_args`, 177 | `n` | Extra option names declared as taking one argument. New options must enter a capability-backed extension schema; this cannot authorize replacing owned inputs, maps, filters or outputs. |
| `audio_tracks`, 187 | `de` | Relative audio indices, all, duplicate removal `d`, empty/near-empty removal `e`, or both. Preserve explicit legacy behavior; modern defaults must not silently remove tracks through heuristics. |
| `audio_codec`, 193 | `aac` | Encoder/copy selection. Aliases include opus→libopus, mp3→libmp3lame, vorbis→libvorbis. Resolve actual encoder and output-container compatibility. |
| `audio_bitrate_kbps`, 197 | `n` | Keep source bitrate when meaningful. A lossless codec has no lossy bitrate setting; mark unsupported options instead of inventing a target. |
| `audio_sample_rate`, 202 | `n` | Preserve source; numeric accepted range is 8,000–192,000 Hz. Validate the selected codec's supported sample rates. |
| `keep_source_metadata`, 212 | `y` | Bundles several preservation choices in donor paths. Split global tags, per-stream tags, chapters, extra video, data, subtitle and attachment policies in the new model. |
| `subtitle_tracks`, 217 | `none` | Drop subtitles in this shipped profile. Map to an explicit stream selector; do not mistake this for a universal application default. |
| `keep_embedded_attachments`, 222 | `n` | Drop attachment streams in this profile. Keep cover pictures and font/file attachments as distinct asset/stream types. |
| `detect_duplicate_audio`, 226 | `y` | Enable duplicate analysis. Record whether results are sampled or exact and require an explicit removal policy. |
| `loudnorm`, 237 | `off` | Configuration supports off/single-pass; interactive paths also provide two-pass. New schema must expose off/dynamic/specified two-pass equally through all clients. |
| `loudnorm_target_i`, 241 | `-16` | Integrated loudness target; donor bounds -30 to -5 LUFS. Explicit TP/LRA targets must also be represented; constants default to -1.5 dBTP and LRA 11. |
| `nvenc_multipass`, 249 | `disabled` | Disabled/quarter-resolution/full-resolution NVENC analysis mode. This is distinct from two complete CPU encoding passes. |
| `cpu_two_pass`, 255 | `n` | Two-pass bitrate encoding for selected CPU encoders, subject to donor builder exclusions. New planner admits only validated encoder-and-graph combinations. |
| `color_range`, 263 | `source` | Source/tv/pc/unspecified signaling policy. Distinguish changing metadata from actual sample-range conversion. |
| `video_speed`, 268 | `n` | Neutral/source or 0.10–8 speed factor. Use a finite positive rational and a defined source/output clock. |
| `reverse_video`, 271 | `n` | Reverse video. Use bounded staging rather than unbounded whole-file frame buffering. |
| `audio_speed`, 275 | `n` | Neutral, factor, or match-video behavior. Match-video also aligns relevant reverse behavior in the donor. |
| `reverse_audio`, 279 | `n` | Reverse selected audio, with bounded staging where required. |
| `logging_enabled`, 288 | `y` | Enable logs. New security/audit events and job state must remain available within the final logging policy. |
| `log_retention_days`, 292 | `0` | Indefinite retention. Do not carry this into an unbounded production default; provide size and age budgets. |
| `log_level`, 299 | `debug` | Verbose donor default. New default should retain complete execution provenance without retaining unlimited repetitive progress lines. |
| `gui_engine`, 304 | `classic` | Python GUI selection (classic/QML). Legacy importer reports it as obsolete; no Python/Qt engine is shipped by Numera. |

#### Runtime defaults that are not configuration keys

The defaults below are observed in `core/constants.py` and encoder helpers. They belong in a documented legacy compatibility profile, not a claim that they are best for every file. [S-constants] [S-encoder-options]

| Concern | Donor behavior | Numera replacement decision |
|---|---|---|
| Default video family | H.265; fallback suggested bitrate 400 kb/s when source bitrate cannot be found | Keep H.265 only as a named profile. Resolve rate control from user objective, source and encoder; no universal 400 kb/s fallback without a visible reason. |
| NVIDIA | Device index 0; NVENC preset p4, tune hq, VBR; HEVC main profile | Probe actual device/driver/encoder/bit-depth path; expose device selection and profile-specific options. |
| CPU encoders | x264/x265 preset medium; SVT-AV1 preset 6 with tune=0 | Named, versioned performance/quality profiles; encoder-specific allowed ranges. |
| Audio | AAC; fallback 128 kb/s; preserve sample rate/channel count where unspecified | Do not collapse multichannel or high-rate audio implicitly. Resolve per output audio stream. |
| Video geometry | Common default pixel formats nv12/yuv420p, forced square-SAR constant, +faststart for relevant containers | Choose bit depth, pixel format, SAR and muxer flags from a documented target profile; preserve source geometry where appropriate. |
| Reverse staging | 60-second maximum segment; default peak budget 2,048 MiB; 512 MiB overhead, 1.15 frame factor | Account for actual decoded format, dimensions, concurrent audio and FFmpeg process overhead. Bound total RSS and scratch disk; reject impossible jobs before execution. |
| Reverse intermediates | CRF 14 picture intermediate, FLAC audio, Matroska stages | CRF 14 is visually high quality, not mathematically lossless. Expose and test an intermediate fidelity policy. |
| Probe parallelism | Duplicate hash workers 2; volume workers 3; folder probe workers 4; sampled audio hash window 8 seconds | Global bounded scheduler; sample-based similarity cannot be presented as full-content identity. |
| Runtime shutdown | Graceful-stop window 8 seconds; bounded stderr tail of 200 lines, last 12 reported | Preserve bounded tails and graceful escalation; add structured logs and subprocess-group termination across platforms. |
| Paths | Legacy default output `E:\output`; Windows launcher/runtime assumptions | Native path APIs and portable data/scratch directories; no drive-letter or PowerShell requirement. |

#### Shared schema required by all fifteen families

The Rust domain model should have serializable types equivalent to the following. These are contracts, not a prescribed implementation dependency:

- `AssetRef { asset_id, content_identity, probe_revision }` and `StreamRef { asset_id, absolute_index, kind, expected_codec }`. Batch selectors use `StreamSelector::{All,TypeOrdinal,Language,TitleContains,Explicit}` and are resolved independently for every asset.
- `TimeSpec::{Seconds(Rational),Frame{index,rate},Sample{index,rate}}`; `TimeDomain::{SourcePresentation,EditedTimeline,OutputPresentation}`. Negative/missing/nonzero source timestamps are normalized through an explicit timeline map.
- `Range { start, end }` is half-open. `Selection::{KeepRanges,RemoveRanges}`, `SplitPlan { points, domain }` and piecewise `SegmentTransform { speed, reverse, effects }` have validated nonoverlap/order rules.
- `ComputePolicy::{CpuOnly,Auto{preferred_devices},GpuRequired{device,required_stages}}`; separate requested and resolved decode/filter/encode devices. A CPU filter plus GPU encoder is an admitted plan, not an erroneous partial-GPU state. “Required” identifies the stages that must use GPU; it must not ambiguously mean every stage, including filters that only have a CPU implementation.
- `VideoRateControl::{Copy,EncoderQuality{value},AverageBitrate{bps,max_rate,buffer,two_pass},Lossless}`. Encoder quality and lossless are distinct; no generic “CRF 0 means lossless everywhere.”
- `StreamPolicy` independently covers video, audio, subtitles, data, attachments, global tags, stream tags, dispositions and chapters, with a per-stream outcome of copy/reencode/convert/retime/drop/reject.
- `OutputClaim { path, format, overwrite_policy, atomic_finalize }` and `ResourceBudget { memory_bytes,scratch_bytes,workers }` describe output/resource intent.
- The canonical request envelope, idempotency contract, immutable plan and job state machine are defined in Sections 4–6 of this mandate. This annex adds behavioral requirements; it does not define a second wire protocol. Processing returns a plan by default; job submission executes the captured plan or reports a precise stale/invalid condition.

CLI, MCP, HTTP, native GUI and browser GUI must deserialize/produce the same operation requests. GUI selection state, terminal prompt answers and browser-local filesystem paths are not implicit backend parameters. The local engine owns files and FFmpeg; a browser receives authorized asset identifiers and preview results.

### F01 — interactive encode, timeline editing and subordinate effects

**Observed entry points:** `run_wizard`; `wizard_steps.py`; `build_ffmpeg_command` in `wizard_build.py`; `build_video_filter` and cut/quick/composite builders. The wizard supports single-source video/audio workflows and optional joining, then configures output location/container, codec, compute preference, edit geometry, bitrate/quality, resolution/FPS, audio selection/normalization/codec, source extras and color signaling. Its graphical editor is a donor interaction mechanism; the underlying editing operations must be retained in the common Rust model. [S-flow] [S-wizard-steps] [S-build]

#### Complete feature inventory inside F01

| Subfeature | Observed suboptions/defaults | Required typed representation and lowering |
|---|---|---|
| Output container | Common video mp4/mkv/mov/webm/avi/m4v/ts; audio mp3/m4a/aac/opus/ogg/wav/flac; wider runtime muxer discovery | Explicit container/codec admission. “Recognized filename extension” must not substitute for a supported codec/muxer combination. |
| Video codec | H.265→libx265/hevc_nvenc; H.264→libx264/h264_nvenc; AV1→libsvtav1/av1_nvenc; VP9→libvpx-vp9; MPEG-4→mpeg4; copy; actual encoder name | Copy has no filters; filtered output needs a selected encoder. Never silently turn strict-copy into H.265. |
| Rate control | Source/explicit bitrate; quality_vbr/strict_size; interactive CRF/CQ default 23 with a donor 0–51 range; CPU two-pass and NVENC multipass | Encoder-specific quality schemas; two-pass measurements/stats are explicit plan stages. Donor generic quality bounds and NVENC lowering need correction. |
| Geometry | Top/left/right/bottom crop; closest-edge scaling; width or height; fit box; explicit stretch; sample-aspect concerns | Ordered geometry nodes with a policy for rotation metadata and chroma alignment; reject a zero/negative result. |
| Timing | Preserve FPS or integer target; global and piecewise cuts/speed/reverse; split destinations | Exact rational timeline and output-time split points; `trim/atrim`, `setpts/asetpts`, `atempo` and concat as required. |
| Rotate/flip | 90° clockwise, 90° counterclockwise, 180°, horizontal flip, vertical flip | Donor recipes: transpose=1, transpose=2, twice transpose=1; hflip/vflip. Account for geometry and rotation tags exactly once. |
| Color adjustments | Gray; brightness -1..1, contrast 0..3, saturation 0..3, gamma 0.1..10 in tables | `eq` and grayscale paths; finite typed values. Gamma exists in tables/builders but is not exposed by the compact token parser. |
| Denoise | light/medium/heavy | `hqdn3d` presets 2:1.5:3:2.25, 4:3:6:4.5, 8:6:12:9. |
| Sharpen | light/medium/heavy | `unsharp=5:5:A:5:5:0` with A=0.5/1/1.5. |
| Blur | light/medium/heavy | `boxblur=R:1`, R=2/5/10. Donor disallows blur+sharpen; Numera may allow an explicitly ordered effect stack if each node is supported. |
| Fade | Fade in/out in seconds | Output-timeline picture fade and selected audio fade; clip durations and speed changes determine placement. |
| Volume | Linear factor, percent, dB; factor 0.01–10 | `volume` after the requested normalization stage. Distinguish gain from normalization and report potential clipping as measurable output behavior. |
| Audio selection | Specific relative tracks, all, duplicate removal, empty/near-empty removal | Resolve selectors to absolute stream IDs; expose analysis evidence and dropped-track outcomes. |
| Audio processing | Codec/copy, bitrate, sample rate, loudnorm off/single/two, speed/reverse/cuts | Per-output-audio graph and measurements; copy is impossible with sample-domain transforms. |
| Extras | Global/stream metadata, chapters, selected subtitles, embedded attachments, additional picture/data streams in applicable paths | Independent policies and a stream outcome ledger; never infer preservation from a blanket “keep metadata” label. |
| Expert options | Declared argv arity, reserved options, alias canonicalization | Structured option scopes with an owned-plan boundary; no second output, hidden input, filter replacement or untracked file access. |

Sources for picture presets/token parsing: [S-look-constants] [S-look-parser]. Sources for volume/raw argv: [S-raw]. Sources for interactive quality and recipe: [S-quality] [S-encoder-options].

**Duplicate/sparse-track details:** `detect_duplicate_audio` first identifies candidate pairs, compares sampled hashes, then performs a **full-duration normalized-audio hash** before marking matching candidates confirmed. The hashes decode to 48 kHz, stereo, signed 16-bit PCM; this is not identity of the original multichannel/high-bit-depth signal. Preserve the full-confirmation stage, and label the normalization used. “Empty” means at most 4,096 encoded bytes; near-empty classification can follow ≤3% of the largest peer when that peer exceeds 1 MiB, ≤12 kb/s for a track at least ten seconds long or with unknown duration, or ≤64 KiB for such a long/unknown track. These are sparse-data heuristics, not decoded silence proof. Numera must not automatically delete a legitimate commentary, alternate-channel or low-bitrate speech track because of them. [S-audio-duplicates] [S-audio-hashes] [S-audio-sparse]

**Interactive loudness suboptions:** off, single-pass and two-pass; optional “measure first”; target integrated loudness; on failed two-pass measurement, retry, explicitly continue as single-pass, or cancel normalization. Declining the prerequisite measurement changes donor two-pass to single-pass. Numera's noninteractive policy must express those alternatives as `measurement_failure:fail|fallback_dynamic` and never fabricate measurements or change mode silently. The baseline target/TP/LRA defaults are -16 LUFS/-1.5 dBTP/11. The first donor interactive measurement is based on the first selected track; new per-track graph measurements are required as specified under F15. [S-loudnorm]

#### Quick outputs: mandatory baseline, not deferred enhancements

`parse_quick_tokens` accepts `gif[=fps[:width]]`, `boomerang`, `thumb`/`thumbnail[=time]` and `loop=N`. GIF defaults are 15 FPS and width 480; thumbnail defaults to PNG and supports JPG/JPEG/WebP/BMP. The donor loop value counts additional repetitions, so the forward/boomerang pair may be rendered N+1 times. The public Numera schema must use unambiguous `repeat_count` and document legacy import arithmetic. A thumbnail cannot be meaningfully combined with repeated output playback. [S-quick] [S-constants]

The donor uses two GIF stages: `palettegen=stats_mode=diff` to a palette artifact, then `paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle`. Preserve the benefit of bounded two-stage execution and identical preprocessing in both passes. A boomerang creates a forward intermediate, reverses it through the bounded pipeline, concatenates the pair and optionally converts to GIF. Every intermediate and palette must appear in the execution plan and owned-artifact manifest.

**Source-confirmed limitations to fix:** `gif_unsupported_answer_notes` warns that multiple cut ranges are ignored and processes the whole clip; reverse GIF can still use a whole-clip reverse graph with only a warning. `build_thumbnail_command` performs source seek and one-frame extraction without the prior edit/filter graph. Numera must render the requested edited timeline for both outputs. It must not accept a crop/rotation/multi-cut request and then silently produce source footage. [S-quick-build]

Proposed operations: `timeline.gif { timeline, fps, width, palette_policy, loop_metadata }`; `timeline.boomerang { timeline, repeat_count, intermediate_profile }`; `timeline.thumbnail { timeline, at:TimeSpec, image_format }`. These use the same planner as a normal export.

#### Compositing and music mixing: mandatory baseline

`parse_composite_tokens` recognizes overlay/logo/watermark, PiP, hstack/sbs/sidebyside, vstack and amix/mix/music. There is **no observed chroma-key feature in FFmWiz**; chroma key may enter through another donor or an explicitly admitted FFmpeg effect, but must not be attributed to this baseline. [S-composite]

| Operation | Donor options | FFmpeg lowering and Numera correction |
|---|---|---|
| Overlay/watermark | top-left/top-right/bottom-left/bottom-right/center; margin 0–4096, default 10; opacity 0–1, default 1; default bottom-right | Partner RGBA/alpha adjustment, `overlay` with repeat-last policy. Define whether short partner media repeats, freezes or disappears. |
| PiP | Same positioning; width fraction 0.05–1, default 0.25 of main width | Scale partner with aspect preservation, then overlay. Reject geometry that violates the chosen bounds policy. |
| Horizontal stack | Partner scaled to main height, aspect preserved | `hstack` after compatible format/time-base preparation; explicit output dimensions and audio policy. |
| Vertical stack | Partner scaled to main width, aspect preserved | `vstack` after preparation. |
| Music/narration mix | Weight 0–4, default 0.3; main weight 1 | `amix=duration=first:weights=1:w:normalize=0`; source duration is retained when the bed is shorter. Represent gain, padding/looping and clipping policy. |

**Source-confirmed limitations:** the composite builder warns that cut/split answers are ignored; it keeps only one selected main audio stream for mixing and can omit other source extras. This is a fractured-builder limitation, not a limitation of FFmpeg. Numera must either compile an admitted combined timeline/stream plan or reject the combination before approval; dropping requested edits is prohibited. [S-composite-build]

#### F01 request, preconditions and acceptance

Primary request: `media.encode { inputs, timeline, video, audio[], effects[], composition, stream_policy, output, compute, budget, expert_options[] }`. Plain transcodes and complex timelines differ in fields, not in a second hidden backend.

Preconditions include readable and unchanged inputs; valid geometry/time ranges; every requested encoder/filter/muxer available in the pinned build; a legal stream/container map; sufficient scratch space for the chosen plan; and validated expert options. Return named errors such as `FILTER_REQUIRES_ENCODE`, `STREAM_CONTAINER_INCOMPATIBLE`, `INVALID_TIMELINE_RANGE` or `RESOURCE_BUDGET_EXCEEDED` with the relevant field/stream, not a generic FFmpeg failure after output creation.

Acceptance scenarios:

- **F01-A:** crop + rotation + non-square SAR + 30000/1001 FPS produces verified dimensions, display aspect and time base, with no double rotation.
- **F01-B:** a GIF from two separated keep ranges contains only those ranges in the correct order; palette and output stages share the same edit graph.
- **F01-C:** a thumbnail after crop/rotation/brightness matches the corresponding decoded rendered-timeline frame within the declared pixel tolerance.
- **F01-D:** overlay + speed change + cuts + output splits preserves requested edit order, duration and all retained audio streams.
- **F01-E:** boomerang + repeat + GIF has the documented frame order/count, bounded RAM and no lost/duplicated seam frame beyond explicit loop policy.
- **F01-F:** negative brightness is accepted within -1..1; NaN/infinity and out-of-range effects are rejected before execution.
- **F01-G:** raw-option aliases, file-loaded filter aliases and valueless-option tricks cannot replace the planned filter, add an input/output or bypass overwrite protection.
- **F01-H:** CPU-only and a GPU-backed admitted plan produce the same requested stream/timeline semantics; unsupported GPU filters produce a disclosed CPU filter stage or a strict-policy rejection.

### F02 — deterministic configuration execution

**Observed:** Mode 2 calls the same wizard with parsed configuration. It preloads a configured source, applies configuration once source metadata is known, seeds the graphical editor and conditionally skips questions. The complete skip table is in `wizard_flow.py:105–348`. It does not provide unattended execution. Some features are prompt-only, and constant-quality video mode has no shipped configuration key. [S-flow] [S-config]

**Numera request:** `config.import.ffmwiz { source, base_directory, compatibility_profile }` returns a versioned native job document plus field-by-field import diagnostics. `jobs.plan { job_document }` and `jobs.submit { plan_id, plan_hash }` execute it without questions. Missing required fields produce a machine-readable validation response; an explicit `--interactive` client may help fill them.

Import every key above, including defaults that remove subtitles/attachments and enable duplicate filtering. Produce a visible policy diff when importing `de` selection or bundled metadata preservation. Preserve unknown legacy lines in an import report, not as silently active engine settings. Keep machine configuration distinct from GUI preferences such as obsolete `gui_engine`.

**Lowering:** exactly the same graph/stream/planner path as the equivalent GUI request. Native configuration may represent two-pass loudness, interactive-only quality mode and composite assets that donor config could not express.

**Acceptance:** **F02-A** importing the supplied forty-key template yields a complete typed profile and documented unresolved paths; **F02-B** CLI/MCP/GUI plans from equivalent parameters have the same semantic plan hash; **F02-C** a missing field on a noninteractive request returns validation immediately and never opens a window or reads stdin; **F02-D** invalid/unknown legacy keys and obsolete GUI values appear in import diagnostics with source line numbers.

### F03 — stream-copy cuts and split files

**Observed:** `run_video_cut_mode` in `modes.py:278–362` accepts a video source, manual timestamps including hours/minutes/seconds/frame notation, keep or remove ranges and an output path. `support/ext07.py:140–249` builds a single copy cut or multiple temporary segments followed by concat. Typical multi-range stages use input seek, duration, `-map 0`, metadata/chapter mapping, `-c copy` and timestamp normalization, with Matroska intermediates. This retains packet data rather than decoding and re-encoding it. [S-copy-cut]

The legacy `h:m:s:frame` parser allows small frame overflow below ten seconds worth of frames and falls back to 25 FPS when a valid FPS is absent. Native Numera frame-based timestamps require an explicit/verified rational rate; they must not silently adopt that fallback for unknown or VFR input. Preserve wall-clock and frame-index entry as distinct forms. [S-timeline]

**Preserve:** one or multiple keep/remove ranges; explicit output format; copied-stream integrity; chapter handling; displayed multi-stage command plan; protection against output/source aliasing. Preserve generated concat/chapter sidecars when the user exports a plan for later execution.

**Correct the contract:** copy cuts have packet/keyframe boundaries, particularly with inter-frame video and open GOPs. They must not be marketed as frame-accurate. New `media.cut { asset, selection, strategy:copy|accurate, boundary_policy, stream_policy, output }` reports requested and resolved boundaries, estimated preroll and copy eligibility. `strategy=copy` must fail with a useful reason if the chosen container/streams cannot safely support the plan; `strategy=auto` may explicitly select an accurate encode path. An arbitrary Matroska intermediate is not a universal compatibility solution for every stream type.

For accurate cuts, decode the selected ranges, normalize timestamps with `trim/atrim` plus `setpts/asetpts`, then concatenate and encode. Specify source-presentation time for cut selection and edited-output time for final split points. Chapters and text subtitles must be transformed by the same timeline map or shown as intentionally omitted; attachments are not time-based and follow a separate policy.

**Failure cases:** invalid/reversed/overlapping ranges, unknown duration required by a remove-range complement, unavailable boundary information, incompatible copied codec/extradata, source modified after planning, output alias/collision or insufficient intermediate space.

**Acceptance:** **F03-A** cut a long-GOP video between keyframes and verify that copy mode reports its packet-boundary behavior while accurate mode meets the declared frame tolerance; **F03-B** a source with nonzero video/audio/container starts keeps AV synchronization after two disjoint ranges; **F03-C** chapters crossing removed ranges are deterministically clipped/retimed; **F03-D** hard-link/symlink/path-alias output claims cannot overwrite any source or generated dependency.

### F04 — folder encoding

**Observed:** `modes.py:451–587` scans media while excluding outputs, uses a representative input for questions, then processes files sequentially. Per-file preparation recomputes metadata, source settings and output paths; failed preparations/encodes contribute to an aggregate failure result. `support/ext11.py:83–159` clears or refreshes file-specific values such as source container, paths, color range and source-derived targets. [S-folder]

**Numera request:** `batch.plan { input_root|assets, recursion, include, exclude, operation_template, output_template, missing_stream_policy, concurrency, failure_policy }` returns a frozen ordered input manifest and a child plan for each file. `batch.submit` executes those ordinary child jobs with default encode concurrency one; bounded overlapping probes may run separately. An explicit recursive policy must be available instead of inheriting inconsistent flat/recursive donor behavior.

**Preserve:** folder convenience; per-file probing; aggregate progress/result; skip reasons; stable naming; no recursive self-processing of output trees; small-memory sequential execution. **Correct:** never reuse source duration, relative-stream resolution, loudness measurements or a fallback decision from one file as though it described every file. A representative file is a UI convenience, not a data model.

**Lowering:** independently compiled FFmpeg argv/stage DAG per source. A batch can contain copy operations and encodes simultaneously if the same declared policy resolves differently per input. The manifest records success, failure, cancellation and skipped files separately; overall success is not the return code of the last process.

**Failure cases:** mixed containers, differing stream layouts, unavailable requested language/track, same-stem naming collisions, output root inside input root, changing files during scan, per-file codec support and partial disk exhaustion.

**Acceptance:** **F04-A** a mixed MP4/MKV/audio folder keeps per-file codec/container and stream decisions correct; **F04-B** two files with very different loudness receive independent two-pass measurement; **F04-C** outputs under the source tree are excluded on initial run and recovered queue dispatch or an explicit new retry; **F04-D** one preparation error, one encode error and one success yield correct child results and a non-success aggregate without losing the successful artifact.

### F05 — append external tracks without changing picture

**Observed:** `_run_add_files_to_video_mode_impl` (`modes.py:626–683`) accepts a video and additional media files, maps the source plus external audio/subtitle streams, applies external stream metadata, and retains the source container. `build_add_files_to_video_command` (`support/L00_misc_b.py:542–574`) performs stream copy. The compatibility gate checks every external audio/subtitle codec against the target container and refuses an incompatible addition before an output is created. [S-add] [S-add-builder] [S-streams]

**Numera request:** `streams.add { target, additions:[{asset,selector,title,language,disposition,offset}], conversion:forbid|allow_declared, stream_policy, output }`. The baseline strict-copy profile uses `conversion=forbid`. Selecting all usable audio/subtitle tracks from an external file reproduces donor behavior; explicit subsets are a natural extension of the same schema. Optional offset is a typed extension and must use tested timestamp mapping if shipped; it is not claimed as an observed donor control.

**Lowering:** explicit `-map` entries for original retained streams and selected external streams; `-c copy`; output-relative metadata/disposition options; independent metadata/chapter/attachment policies. Strict-copy addition must not silently convert SRT into MP4 timed text or re-encode audio. A conversion-capable Track Manager operation is a separate admitted policy, even if both share the planner.

**Failure cases:** unsupported subtitle/audio codec in the source container, external file with no usable streams, stale stream IDs, invalid language/disposition, planned output aliasing an external source, timestamp offset outside the admitted policy.

**Acceptance:** **F05-A** append two audio tracks and two text subtitles to a multistream MKV and verify original packet payloads, stream order, language and titles; **F05-B** add an incompatible subtitle under strict-copy policy and verify rejection before output creation; **F05-C** a source with cover art, chapters and fonts retains exactly the declared extras; **F05-D** metadata indices remain correct after selection changes and are never applied to the preceding original track.

### F06 — extract selected streams

**Observed:** `modes_b.py:99–162` supports a file or folder and multiple absolute ffprobe stream indices. It groups extraction jobs by source into one FFmpeg invocation with several output clauses, avoiding repeated reads of the entire input. Candidate streams are video/audio/subtitle, not generic attachments. `build_extract_jobs` produces an `_Extracted/<filename.ext>/...` hierarchy for multi-output work and allows a container override for one source/one stream. Each output receives its own map and codec/negative-stream options. [S-extract] [S-extract-jobs] [S-streams]

**Numera request:** `streams.extract { assets, selector, per_stream_format, strategy, output_template, missing_stream_policy }`. Persist both a logical selector and its resolved per-file stream identity. A global index from one file is not safely transferable to another; batch semantics must be explicit.

**Lowering:** one input and multiple independently scoped output clauses where resource/error isolation permits. Each clause has its own `-map 0:<absolute-index>`, selected copy/conversion codec and output container. Native copy formats or supported fallback containers may be proposed; Matroska is a useful option, not proof that an arbitrary codec will mux. Subtitle text conversion is a disclosed conversion; bitmap subtitle OCR is not part of this baseline.

**Output contract:** each artifact has its own finalized/partial/failed state. A successful output is not discarded merely because a later output fails, but the parent job cannot claim complete success. Attachments/cover extraction use explicit `attachments.extract`/`cover.extract` registry actions if exposed; do not falsely claim they are present in donor F06.

**Acceptance:** **F06-A** one invocation extracts chosen video, audio and subtitle streams with correct output-scoped options and verified stream identity; **F06-B** folder extraction reports files lacking the requested selector under skip/fail policies; **F06-C** bitmap/text subtitle format mismatches reject or explicitly convert according to policy; **F06-D** failure creating a second destination produces accurate per-artifact state and never exposes a partial file as complete.

### F07 — inspection, analysis and reports

**Observed:** `modes_mediainfo.py` exposes file/folder reporting, optional deep analysis, optional reference comparison and optional screenshots. It saves text, HTML and raw JSON; deep analysis includes packet/frame CSVs. The report includes probe metadata, stream-size/bitrate analysis, audio volume information, quality comparisons and screenshots at 10/25/50/75/90 percent. Reference comparison is a single-file option and can use PSNR, SSIM and VMAF when available. [S-report]

**Important implementation evidence:** `support/ext09.py:196–254` performs exact packet-size scanning even when “deep” is false. `support/ext01.py:312–456` captures complete CSV stdout, splits it into lines and stores all frame rows before analysis. On long/high-frame-rate media this is not bounded-memory reporting. Comparison checks warn about timing/geometry/rate mismatch but may continue, so an apparent metric number is not automatically a valid comparison. [S-report-scan] [S-report-csv] [S-metrics]

**Numera requests:**

- `media.probe { asset, detail:basic|streams|chapters }` is a bounded metadata operation with timeouts.
- `media.report { asset|batch, analyses:[stream_sizes,volume,packets,frames,keyframes,screenshots], output_formats, budget }` explicitly declares full scans.
- `media.compare { reference,test,streams,alignment,normalization,metrics }` admits a metric only when its frame/timestamp/color assumptions are satisfied.

**Lowering:** ffprobe JSON for structure; targeted packet/frame queries streamed to on-disk CSV and incremental accumulators; selected FFmpeg `volumedetect`/`astats` or the documented analyzer; one-frame extraction at resolved screenshot positions; `psnr`/`ssim`/`libvmaf` on a deliberately aligned graph. Never concatenate unbounded process output into a string. For live/unknown-duration input, percent screenshots cannot be inferred without a completed duration policy.

Report fields carry provenance: declared, measured, estimated or incomplete. “Exact stream bytes” requires a completed packet scan. Sampled silence/duplicate/color conclusions disclose sampling limits. HTML escapes metadata and path text; reports never execute source-supplied script content.

**Acceptance:** **F07-A** basic inspection of a long media file avoids an implicit full scan; **F07-B** full packet/frame reporting stays within the declared memory budget while CSV size grows on disk; **F07-C** a one-frame offset or resolution/color-space mismatch cannot yield an unlabeled “comparable” metric; **F07-D** malicious HTML-like metadata is inert in an opened report; **F07-E** cancelled/truncated analysis marks incomplete measurements rather than presenting them as exact.

### F08 — cleanup, selection and remux

**Observed:** the `muxcleanup` subsystem supports single-file and folder modes, advanced or exact-index selection, independent audio/subtitle choices, attachment/metadata/chapter preservation, overwrite behavior, optional copying of non-video files, output verification, metadata edits and stream order. Audio/subtitle selectors are all, none, exact indices, language or title substring; matching normalizes language codes and compares title substrings case-insensitively. Partial stream-order lists place the listed kept streams first and preserve the remaining relative order. [S-clean-model] [S-clean-select] [S-clean-build]

The metadata/order submenu can set audio or subtitle language by current language, set language by exact indices, set title by exact indices, reorder either kind, clear accumulated edits/order, and finish. Edits apply to retained output streams. This is a real subfeature of cleanup, separate from F13. [S-clean-edit]

**Numera request:** `streams.clean { assets, audio_selection, subtitle_selection, audio_order, subtitle_order, stream_tag_patches, preservation, output_tree, copy_non_media, overwrite, verify }`. It should share metadata/selectors with F13/F15; no second implementation of language or stream-index semantics.

**Lowering:** explicit retained video/audio/subtitle/attachment maps, `-c copy`, separate `-map_metadata` and `-map_chapters`, then output-indexed metadata/disposition options. The donor maps all video streams but does not explicitly preserve data streams here. Numera's outcome ledger must state the fate of each data/extra stream instead of implying all original streams survive.

**Preserve:** exact-copy optimization when nothing changes; remux when stream selection/order/metadata/chapters differ; directory-tree preservation where selected; independent output verification; aggregate failure reporting. Replace Windows `robocopy` dependencies with Rust filesystem operations. Derive no-op from the complete resolved operation, including ordering and metadata, not a few selection flags.

**Acceptance:** **F08-A** select audio by language and subtitles by case-insensitive title, partially reorder, and verify output indices/tags and unchanged picture payload; **F08-B** metadata-only, chapters-only and order-only changes each trigger a real remux; **F08-C** all retained dispositions are preserved without silently assigning a new default; **F08-D** data streams and embedded covers/fonts follow explicit policies; **F08-E** recursive tree copying excludes its output subtree, respects overwrite policy and records non-media copy failures.

### F09 — hard subtitles

**Observed:** internal text subtitles or external SRT/ASS/SSA/VTT/WEBVTT, optional font directory, audio all/selected/none, output container and quality profiles. Bitmap subtitle input is rejected by this text/libass workflow. Quality choices are near-lossless/high/balanced/custom, with custom values 1–51; constants vary by encoder (typical CPU 14/17/20, x265 16/18/22, NVENC 13/16/20). HDR policy choices include preserve best effort, tone-map to SDR and standard processing. [S-hardsub-options] [S-hardsub-build]

When the target container differs, the donor additionally asks to copy audio anyway, transcode selected/copied audio to AAC, change the output container back to match the source, or remove audio. Default is the AAC route. The prompt labels it AAC stereo while the builder uses its shared channel resolver, so the new plan must show the actual resolved channel layout instead of inheriting a potentially misleading prompt. “Copy anyway” becomes a validated strict-copy request; an incompatible result is rejected in preflight rather than knowingly issuing an invalid header. [S-hardsub-audio-policy]

**Numera request:** `subtitles.burn { timeline, subtitle:{embedded_stream|external_asset}, fonts, style_policy, quality_profile, color_policy, audio_policy, output, compute }`. Font files and subtitle assets become declared input dependencies. A normal browser file picker must upload/register the chosen asset with the local engine, not ask FFmpeg to read a fictitious browser-local path.

**Lowering:** a libass-capable `subtitles`/`ass` graph on decoded picture frames, plus the chosen video encoder. CPU subtitle rendering and NVENC encoding is a valid mixed plan. Preserve non-square SAR deliberately. Map every selected audio stream independently, validate each codec against the target container and preserve/convert only under the selected policy. “The first audio track is compatible” is insufficient for a multilingual file with heterogeneous codecs.

**Source-and-manual-confirmed correction:** `build_hardsub_video_filter` places `t=bt709` before CPU `tonemap` in its SDR conversion chain. FFmpeg's `tonemap` expects single-precision floating-point, linear-light data. The correct schematic is explicit source characterization → `zscale=t=linear:npl=<policy>` → `format=gbrpf32le` → primaries conversion while transfer remains linear → `tonemap` → target transfer/matrix/range conversion → target pixel format. Resolve peak/desaturation from a documented profile; do not universalize one number. Set output color tags after conversion and remove stale HDR metadata. The project's pinned FFmpeg manual is the authority for the concrete chain. [S-hardsub-build] [S-ffmpeg-tonemap]

HDR “best effort” must state whether HDR10 metadata survives and whether dynamic metadata is unsupported. Re-encoding must not claim Dolby Vision or HDR10+ fidelity merely because the output is 10-bit.

**Acceptance:** **F09-A** Persian/Arabic/RTL text, Unicode paths and a required external font render correctly in a known subtitle fixture; **F09-B** HDR PQ/HLG to SDR produces correct transfer/range tags and reference pixels, without stale HDR metadata; **F09-C** heterogeneous audio tracks are individually copied/converted/rejected as declared; **F09-D** bitmap input returns a clear unsupported-text-workflow error rather than an unusable output; **F09-E** CPU and GPU-encode variants preserve subtitle timing and geometry within the profile's visual tolerance.

### F10 — video speed and reverse

**Observed:** a standalone video-only entry point accepts speed/reverse choices, then renders the primary video with H.264 and source audio with AAC. Its container list excludes WebM because this builder uses H.264/AAC. The simple builder maps the first video, processes source audio, drops subtitle/data output and uses CPU libx264 medium CRF 18. This is a donor builder constraint, not a reason to deny a validated VP9/Opus WebM retime in Numera. Long reverse work uses the dedicated segmented pipeline. [S-transform] [S-transform-build] [S-reverse]

**Numera request:** `media.retime { timeline, segments:[{range,speed,reverse}], audio:match_video|independent|drop, subtitle_timing, chapters, encoder_profile, output, compute, budget }`. No-op is a structured result when factor is one and reversal is false. `atempo` preserves pitch; a pitch-changing alternative is not an observed baseline requirement and needs its own explicit effect if added.

**Lowering:** forward retime uses normalized `setpts`; selected audio uses a correctly factored `atempo` chain and timestamp treatment. Do not let implicit source-rate synchronization drop frames from a retimed VFR picture. Reverse stages must reverse content within bounded chunks and reverse chunk order, preserve exact stream/time boundaries and account for output order in final splitting. Audio may require separate lossless staging. Reversing sixty seconds of 8K floating-point video can still exceed a small memory budget; duration alone is not a budget.

**Stream policy:** preserved subtitles/chapters must be retimed by the same source-to-output mapping or explicitly rejected/dropped by policy. Attached pictures are not ordinary timeline video. Multi-angle video must have a documented outcome.

**Acceptance:** **F10-A** 0.1×, 1× and 8× cases meet duration, picture-frame and audio-sample tolerances without unintended pitch change; **F10-B** reversing labeled frames and channel impulses across multiple chunk boundaries proves correct chunk order and no seam loss; **F10-C** reverse + speed + final-timeline splits produces the expected media segments; **F10-D** an 8K case that cannot fit the budget is segmented more finely or refused before runaway allocation; **F10-E** VFR timestamps stay monotonic and do not collapse to an unrequested CFR.

### F11 — audio cut, speed and reverse

**Observed:** source/track selection, keep/remove cutting, single or multiple ranges, audio speed and reversal, with graphical editing or terminal fallback. `build_audio_cut_command` can use copy for an eligible single range; multiple ranges use `asplit`, `atrim`, `asetpts` and `concat` and therefore encode. Copy boundaries are packet-accurate, not sample-accurate. Speed factors span 0.10–8 and use tempo filters; long reverse uses bounded lossless staging. Source sample rate/channels are preserved when unspecified. Cover-art handling depends on output container. [S-transform] [S-transform-build] [S-audio-reverse]

**Numera request:** `audio.edit { asset, stream, selection, pieces, speed, reverse, gain, normalization, audio_profile, cover_policy, output, budget }`. A piece can carry its own transformation; chronological order and reset points are explicit. Normalize the audio that is actually exported after selection/mixing/retiming, not a different unedited source signal.

**Lowering:** single copy trim uses a tested seek/duration policy; accurate single/multi-range edits are sample-domain graphs. Construct one output label per retained audio output. Bounded reverse determines chunk length from sample rate, sample format and channel count; intermediates preserve the required precision. A lossy source-derived bitrate must not force an arbitrary 128 kb/s target onto a lossless output. Audio transform + `copy` is a validation conflict or an explicit profile change.

**Failure cases:** nonexistent audio stream, invalid/empty ranges, unsupported channel layout/sample rate, unrepresentable requested precision, incompatible cover-art format, insufficient scratch, or reverse on an unbounded input without an admitted materialization limit.

**Acceptance:** **F11-A** multirange edits of a numbered impulse fixture prove sample order and cut precision; **F11-B** 5.1 audio retains channel identity and target sample rate through retime/reverse; **F11-C** lossless output matches the declared sample precision rather than an undocumented lossy intermediate; **F11-D** copy/accurate modes expose different boundary guarantees and never label packet cuts sample-accurate; **F11-E** cover handling and output audio stream count match the declared plan.

### F12 — joining video or audio

**Observed:** ordered inputs, optional folder append, minimum two files, duplicate-path rejection, all-video or all-audio requirement, mixed video/audio-only rejection. Different video frame rates offer unified CFR or preserved VFR. Compatible streams use concat copy; otherwise audio joins encode AAC with the highest known source bitrate, and video joins offer a near-quality encode. The donor's stream signature compares codec/geometry/FPS/pixel format/SAR/profile/level/field order for video and codec/sample rate/channels/layout for audio. [S-join] [S-streams]

**Numera request:** `media.join { inputs_in_order, track_mapping, strategy:copy|encode|auto, frame_rate_policy, missing_track_policy, geometry_policy, stream_policy, output_profile }`. This operation shares the timeline compiler with F01. The interface can preserve simple “join these files” convenience while exposing the exact resolved track correspondence.

**Lowering:** concat demuxer + explicit maps for a proven compatible copy path; normalized per-segment decoded streams and concat filter for incompatible sources. Check stream layout, codec parameters/extradata, time base and packet behavior, not just extensions or rounded FPS. Define whether missing audio is silence and how later-only language tracks are represented; the donor has specific support for joined track views and silence preparation that must not regress. A stream only present in input two must not disappear because input one lacks it.

The donor `join_copy_plan` can preserve copy when only some tracks are selected, but explicitly disabling all audio forces an encode workaround in one path. This is not an inherent FFmpeg limitation: explicit retained-video maps can keep a valid copy join without audio. Chapters, text subtitle offsets and metadata ownership need a resolved policy across every segment, not accidental inheritance from the first file. Generated concat files are owned plan artifacts and remain available when exporting a manual plan. [S-join-policy] [S-join-audio]

**Acceptance:** **F12-A** compatible streams with audio dropped remain picture-copy eligible and preserve packet payloads; **F12-B** first input silent, later input with multiple audio languages produces the declared silent gaps and all logical output tracks; **F12-C** differing SAR/extradata or time-base cases cannot be incorrectly advertised as safe copy; **F12-D** VFR-preserving and explicit-CFR joins have distinct verified timing outcomes; **F12-E** chapters/subtitles start at correct concatenated offsets and exported commands retain their sidecars.

### F13 — metadata, dispositions, chapters, covers and signaling

This menu contains five subfamilies and must not become a single vague “edit metadata” requirement. [S-metadata]

| Subfamily | Complete observed controls | Numera schema/lowering |
|---|---|---|
| Stream metadata | Set/remove title, set/remove language, set/remove a custom metadata key on a selected real stream | `metadata.patch { target, scope:global\|stream, set:{}, remove:[] }`; explicit output-scoped metadata with stream copy. Global patch support is a typed extension; donor also handles global metadata through ffmetadata import. |
| Dispositions | Audio default set/remove; subtitle default set/remove; subtitle forced set/remove; commentary set/remove; original set/remove; clear all; custom flag editor; optional exclusive default within kind | `disposition.patch { stream, add:[],remove:[],clear_all,exclusive_default }`; preserve unrelated flags when setting/removing one bit. |
| Chapters | Show, export FFmetadata, import metadata+chapters, remove all | `chapters.inspect/export/import/remove`; validated rational start/end/timebase/title and explicit global-metadata import policy. |
| Cover/attached picture | Add, replace, remove, extract, with container-specific embedded-image representation | `cover.add/replace/remove/extract`; distinguish attached-pic video from Matroska file attachments and update only the selected cover. |
| Video signaling | Inspect declared color tags; estimate actual range at every 120th/30th/5th frame; set H.264/HEVC full-range flag, color primaries/transfer/matrix and SAR | `video-signaling.inspect/estimate/patch`; compatible `h264_metadata`/`hevc_metadata` bitstream filter, with codec-specific numeric/enumeration validation. |

The range estimator can ask for CUDA/NVDEC decoding while `signalstats` still runs on CPU; it is a sampled heuristic, not proof of the original intended range. A metadata-only patch does not convert pixel values. Represent `SignalOnly` separately from `ConvertPixels` and make its result unambiguous in GUI/CLI/MCP.

**Source-confirmed disposition problem:** the donor uses a bare positive flag such as `default`, which replaces the current disposition set rather than adding one flag, and the exclusive-default branch emits `-disposition:<kind> 0`, clearing all flags for that kind. Numera must preserve commentary/original/forced and any unrelated flags unless removal/clear-all was requested. Use an explicit resolved bit set or supported additive/removal syntax. [S-dispositions]

**Failure cases:** unsupported bitstream codec, invalid SAR/standard code, stale selected stream, ffmetadata chapter intervals outside the timeline, cover codec/container incompatibility, malformed or oversized imported metadata, attempted in-place mutation of a currently used input.

**Acceptance:** **F13-A** setting one default flag preserves existing commentary/original flags on all relevant streams; **F13-B** chapter export/import round-trips rational timing and Unicode titles without changing unrelated global tags unless requested; **F13-C** adding/replacing a cover does not remove the primary video or unrelated font attachments; **F13-D** signaling-only range changes leave decoded sample values unchanged while tags/bitstream fields change; **F13-E** sampled range estimation reports sample density, confidence limitations and incomplete scans honestly.

### F14 — capabilities and cache maintenance

**Observed:** view, re-probe and clear cache. The re-probe path exercises color-range behavior across h264/x265/h264_nvenc/hevc_nvenc and mp4/mkv combinations. Cache identity includes executable/environment information (see `capability_environment_identity`, `services.py:137–181`), and GPU availability includes a short real encoder operation rather than relying solely on advertised encoder names. The NVIDIA smoke probe uses a 256×256 lavfi input, roughly 0.1 seconds/one frame and a six-second timeout. Cache clearing is scoped to owned JSON/corrupt backups. [S-capabilities] [S-capability-identity] [S-nvidia]

**Numera requests:** `capabilities.inspect { scope }`, `capabilities.refresh { devices,components,force }` and `capabilities.clear { namespace,fingerprint }`. Return separate advertised, successfully exercised, unavailable, failed and unknown states. A timeout or transient driver issue is not permanent proof of unsupported hardware.

**Required cache key:** actual FFmpeg/ffprobe identity and configuration/build fingerprint; operating system/architecture; GPU device/driver; runtime-dependent library state; capability schema version; relevant option profile. Invalidate changed executables/devices. Include filters, encoders, decoders, muxers, pixel/sample formats, subtitle/fonts requirements and exact encode/filter/device paths used by registered operations.

The runtime must distinguish CPU-only, auto preference and strict GPU. GPU decode, filter and encode eligibility are separate. A `scale_cuda` path that is valid for a simple resize may be invalid for libass, a CPU-only effect, multicut, reverse or a complex composition. An admitted mixed graph uses explicit upload/download boundaries; a forbidden fallback returns a preflight error. Do not advertise “GPU support” solely from `ffmpeg -encoders`.

**Acceptance:** **F14-A** a listed but unusable NVENC encoder is reported as advertised-but-failed and auto policy can select a disclosed CPU plan; **F14-B** changing FFmpeg executable or driver invalidates the correct cache entries; **F14-C** clearing a namespace leaves unrelated files untouched; **F14-D** CPU-only planning never starts a GPU probe/encode as a hidden prerequisite; **F14-E** strict GPU fails clearly when a required stage has no admitted path, while mixed auto processing retains every requested effect.

### F15 — track removal, addition, replacement and loudness

**Observed:** single-file or folder scope; removal by absolute index or type-relative `a:1`/`s:0`; folder mode requires type-relative selectors because absolute indices differ; all external audio/subtitle streams can be appended; remove+add gives replacement; loudnorm off/single/two-pass; metadata keep/remove. Folder traversal here is flat and skips names recognized as generated outputs. The command maps source streams, subtracts selected streams, appends external streams and recalculates output metadata indices. Unlike F05, text subtitles may be converted for the output container; incompatible bitmap subtitles are rejected before processing. [S-track-manager] [S-track-builder]

**Numera request:** `streams.replace { assets, remove, additions, conversion_policy, normalization_by_stream, metadata_policy, output }`, plus simpler `streams.remove` and `audio.normalize` actions that compile through the same operation type. A folder request resolves removals and external-output indices independently per file.

**Two source-confirmed issues to fix:**

1. `_track_manager_ask_loudnorm` explicitly measures a representative first file in folder two-pass mode and reuses that result for all files. The builder can also apply one measured loudnorm chain through an unqualified `-filter:a` to every output audio stream. Correct two-pass statistics belong to each final audio signal, identified by source content, selected streams and upstream cut/mix/retime/gain graph.
2. The no-op guard checks only removals, additions and loudness. Choosing metadata removal alone can therefore return “nothing to do.” No-op detection must compare the complete resolved stream/metadata/chapter/disposition operation.

**Lowering:** explicit source/output stream maps and conversion overrides; one normalization measurement/apply pair per output audio stream where two-pass mode is selected. Streams kept as copy remain copy unless they have their own sample-domain transform. Converted subtitles and re-encoded audio appear individually in the outcome ledger.

**Acceptance:** **F15-A** replace the second audio stream and append subtitles with correct output metadata after reindexing; **F15-B** two files and two audio tracks with substantially different loudness each meet the target according to independent output measurement; **F15-C** metadata-only cleanup performs a real remux and removes the requested tags/chapters; **F15-D** folder type-relative removal resolves correctly across differing global stream layouts; **F15-E** copy-preserved video/other audio remains unmodified while exactly the selected normalized tracks are encoded.

### Mandatory migration corrections, separated from feature parity

These are concrete source findings. A source-confirmed branch is evidence of donor behavior, not a claim of a newly executed reproduction. An implementation ticket should retain this evidence class and cite the same commit.

| ID | Evidence and location | Required correction / proving scenario |
|---|---|---|
| D01 | Observed: `run_wizard` retains editor/final prompts even in config mode, `wizard_flow.py:105–348` | Native config execution is truly noninteractive. F02-C. |
| D02 | Source-confirmed: GIF multi-cut warning explicitly selects whole clip; `wizard_build_c.py:524–544` | Compile full requested edited timeline into both palette stages. F01-B. |
| D03 | Source-confirmed: raw GIF reverse still buffers the whole clip; same function | Use bounded reverse staging or refuse the job within its declared budget. F01-E/F10-D. |
| D04 | Source-confirmed: thumbnail command omits the edit graph; `wizard_build_c.py:563–577` | Render the chosen edited frame. F01-C. |
| D05 | Source-confirmed: composite builder ignores cuts/splits; `wizard_build_c.py:328–342` | One timeline compiler for composites and ordinary edits. F01-D. |
| D06 | Source-confirmed: composite graph takes one selected main audio stream and warns about the rest; `wizard_build_c.py:203–325` | Explicit independent retained-track policy; no unrequested stream loss. F01-D. |
| D07 | Source-confirmed: folder two-pass loudnorm reuses first-file measurement; `trackmanager.py:516–541` | Per-file, per-final-signal measurements. F15-B. |
| D08 | Source-confirmed: track builder applies common audio filter settings across outputs; `support/ext01b.py:332–432` | Separate measured statistics and scoped filters for each output track. F15-B. |
| D09 | Source-confirmed: metadata-only track-manager change is discarded by no-op guards; `trackmanager.py:632–634` and `732–734` | Compare complete resolved operations. F15-C. |
| D10 | Source-confirmed: set/exclusive-default disposition commands clear unrelated flags; `metadata.py:195–267` | Patch only requested bits; clear-all remains an explicit operation. F13-A. |
| D11 | Source + pinned FFmpeg manual: hardsub SDR chain converts to BT.709 transfer before `tonemap`; `wizard_build_subtitles.py:101–142` | Keep linear floating-point data through tonemap, then output conversion/tagging. F09-B. |
| D12 | Source + pinned FFmpeg NVENC implementation: `-rc constqp -cq Q` at `wizard_base.py:144–150` / `wizard_build.py:573–584` | Separate VBR-CQ from constant-QP. Lower VBR-CQ with `-rc vbr -cq Q` plus the selected ceiling policy; lower constant-QP with `-rc constqp -qp Q` or explicit frame QPs. |
| D13 | Static risk: generic “0–51; 0=lossless” quality UI and generic CPU `-crf` for selected encoders; `support/ext01c.py:274–321` | Encoder-specific range, mode and lossless semantics; unknown quality mode is not accepted because a numeric value looks valid. |
| D14 | Source-confirmed inconsistency: look token parser rejects negative numeric tokens while brightness tables allow -1..1; `support/L01_filters.py:79–107` | Central effect schema used by every client/parser. F01-F. |
| D15 | Static numeric-validation risk: quality parser compares float bounds without a finite-value check | Reject NaN/infinity throughout schemas, not only in a GUI slider. F01-F. |
| D16 | Source-confirmed: full packet scan during ordinary report and whole-stdout/frame-row accumulation; `support/ext09.py:196–254`, `support/ext01.py:312–456` | Explicit full-scan jobs; streaming parsing and bounded accumulators. F07-A/B. |
| D17 | Static correctness risk: packet CSV parser scans candidate numeric values instead of stable named fields; `support/ext01.py:312–370` | Parse known typed fields, handle absent/N/A values and malformed rows without guessing a packet size from an unrelated number. |
| D18 | Source-confirmed: metric mismatch checks can warn and continue; `support/ext04.py:282–342` | Admit a metric only after explicit alignment/normalization, with validity metadata. F07-C. |
| D19 | Source-confirmed limitation: audio-off copy join becomes encode in a donor policy branch; `support/ext01d.py:361–471` | Prove compatible video-only maps and retain copy where valid. F12-A. |
| D20 | Static compatibility risk: hard-sub audio compatibility begins from a representative first audio stream; `wizard_build_subtitles.py:215–218` and `229–232` | Validate every retained stream against the target container and codec profile. F09-C. |
| D21 | Observed implementation constraint: CPU two-pass first stage maps `0:v:0` and is excluded for multiple complex workflows; `support/L02_encoder_policy.py` | Derive both passes from the same compiled video graph, or declare an unsupported encoder/graph combination before execution. Never append two-pass flags to an incompatible graph. |
| D22 | Observed policy debt: shipped `de` audio selection, subtitle none and attachment drop can remove information; `config.env.example` | Legacy profile preserves user-intended settings with an explicit diff; new default policies do not silently delete tracks through heuristic detection. |

For D12, the independently inspected FFmpeg pin is `n9.0.2` at `946fcce07b6dcd0331c8cc609192aeff5e1924f8`. The NVENC option tables describe `cq` as VBR target quality, while the constant-QP setup uses `qp`/initial QP fields. Merely setting `targetQuality` does not change an explicitly selected constant-QP rate-control mode. This is a semantic recipe error, not a naming preference. [S-ffmpeg-nvenc] [S-ffmpeg-nvenc-h264] [S-ffmpeg-nvenc-hevc]

#### Already-fixed donor behaviors that must not regress

Source comments and current code show that several previous failure modes have already been corrected. Preserve these protections as requirements rather than rediscovering them:

- Raw options are tokenized into argv; aliases and file-loaded filter forms are canonicalized; known arity prevents an unnoticed second output. [S-raw]
- Manual-plan export retains generated join/metadata dependencies so the displayed command remains executable. [S-join]
- Multi-output extraction scopes options per output and shares a source read where practical. [S-streams]
- Joined track views include later-input-only audio/subtitles, rather than taking the first input as the complete catalog. [S-flow] [S-join-audio]
- Copy-join signatures include SAR/profile/field order rather than only size and codec. [S-streams]
- Folder results aggregate failures across the entire batch. [S-folder] [S-track-manager]
- Bounded reverse and audio-reverse staging already exist; do not regress to a single whole-file `reverse`/`areverse` graph for convenience. [S-reverse] [S-audio-reverse]

### CI acceptance framework for this donor baseline

The numbered scenarios in F01–F15 are implementation requirements. They are independent behavioral checks; command-string snapshots alone cannot prove any of them. No tests were run as part of this research.

#### Small deterministic fixture catalog

Generate redistributable fixtures in CI from lavfi, authored subtitles/metadata and small checked-in manifests:

| Fixture | Required properties | What it proves |
|---|---|---|
| MX1 multi-stream | Two ordinary video streams where container allows; multiple audio languages/layouts; text and bitmap subtitle fixtures; cover; font attachment; data where supported; chapters; default/forced/commentary/original flags | Explicit mapping, preservation, output reindexing and incompatible-stream handling. |
| TM1 clocks | Nonzero video/audio/container starts, B-frames and long/open GOP cases | Picture clock versus seek clock, packet-copy boundaries and synchronization. |
| TM2 rates | CFR 30000/1001, true VFR and differing join-segment rates | Rational time arithmetic, VFR preservation and explicit CFR behavior. |
| PX1 picture | Numbered frames, unique border pixels, rotation tags, non-square SAR, odd geometry where supported | Correct cut/order/resize/rotation and chunk seams. |
| CL1 color | Controlled SDR full/limited-range ramps plus 10-bit PQ/HLG fixtures | Pixel conversion versus signaling, tone-map ordering, stale-metadata removal. |
| AU1 audio | Numbered/located impulses, per-channel tones, 5.1, silent/near-silent tracks, distinct loudness per stream/file | Sample order, pitch, layout, loudnorm independence and heuristic limits. |
| CP1 composition | Long main clip, short secondary clip, still logo, short music bed | Duration/EOF/loop policies and retained-track behavior. |
| FS1 paths | Persian/Unicode names, spaces, quotes, commas/semicolons/colons where legal, long Windows paths, hardlinks/symlinks and output/input aliasing | Shell-free argv, graph/path escaping, dependency ownership and portability. |
| ER1 failures | Truncated media, missing stream, corrupted cache, unavailable encoder, output collision and controlled disk exhaustion | Preflight failures, atomic finalization, cancellation and truthful result aggregation. |

Generated fixture lengths stay small in per-PR CI. Long-duration and 4K/8K memory workloads belong in bounded scheduled/release suites; they need measured process RSS, not a claim that Rust itself makes FFmpeg cheap. GPU scenarios run only on the user's explicitly configured local hardware/self-hosted runner; they must not be required on generic hosted runners or trigger local GPU use without that configured workflow.

#### Required oracles

Use ffprobe for output structure and timing, independent decoded-frame/sample checks for timeline/transform semantics, and an independently constructed reference where needed. Packet/bitstream preservation checks should account for admitted container/bitstream-header normalization instead of falsely failing every legal remux. Loudness assertions use fresh output measurements, not the same stats fed to the encoder. Subtitle/font checks use reference images and timing windows. Memory assertions measure all owned FFmpeg/ffprobe/engine processes and scratch usage.

Each operation's registry metadata must point to:

1. At least one valid CPU fixture and one invalid/preflight case.
2. Stream/container outcomes and the expected time-domain behavior.
3. GUI/CLI/MCP/HTTP request-equivalence coverage.
4. Cancellation, output-finalization and log/command-preview coverage where the action creates artifacts.
5. Device variants only where the capability registry admits them.

#### Cross-family cases that isolate architectural mistakes

These interactions are mandatory because the donor's split builders fail precisely at their boundaries:

- Multi-cut → effect stack → GIF palette and GIF encode.
- Crop/rotate → thumbnail at an edited-timeline time.
- Overlay/music → cuts → speed/reverse → final-timeline splits.
- Join with first source silent and later source holding the only selected audio/subtitle language.
- Join compatible video while dropping all audio, retaining copy.
- Replace tracks → per-track loudnorm → metadata removal.
- Metadata-only and order-only edits with no track removals.
- Reverse segmented under a small budget → split → cancel → new linked retry/replan; the cancelled job remains terminal.
- Export a multi-stage command plan → close the client → execute with its preserved owned sidecars.
- Automatic GPU fallback → re-resolve/show actual plan and log the reason; never claim the originally displayed argv was the executed argv.

The implementation may have more tests, but counting tests is not the success criterion. Every documented behavior must have an independent output or failure oracle, and the expensive GPU gate must remain aligned with the user's explicit local-only GPU testing constraint.

### Source index and artifact index

All FFmWiz source links below are pinned to the inspected commit, not a moving default branch. Exact function-level indexes and source hashes are in `ffmwiz-source-inventory.json`. The local mirror is evidence for this appendix, not vendored application code or a decision to copy its implementation.

- `ffmwiz-deep.md` — this behavioral migration specification.
- `ffmwiz-source-inventory.json` — 129 files, source hashes, 1,354 function spans and all forty template keys.
- `ffmwiz-workflow-baseline.json` — compact operation/fixture/acceptance index derived from this report.
- `sources/ffmwiz/` — pinned non-GUI source mirror.
- `ffmpeg-engine-deep.md` and `ffmpeg-upstream/` — independently researched FFmpeg semantics and source/manual snapshot used for the HDR/NVENC corrections.



[S-add]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/modes.py#L626-L683
[S-add-builder]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/L00_misc_b.py#L542-L574
[S-audio-duplicates]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext09.py#L118-L193
[S-audio-hashes]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext00c.py#L354-L442
[S-audio-reverse]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext04c.py#L128-L554
[S-audio-sparse]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext08.py#L157-L197
[S-build]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_build.py#L217-L728
[S-capabilities]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/modes.py#L107-L186
[S-clean-build]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup/muxlogic.py#L20-L264
[S-clean-edit]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup/metadata_edits.py#L87-L230
[S-clean-model]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup/models.py#L72-L114
[S-clean-select]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/muxcleanup/selection.py#L1-L400
[S-composite]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_composite.py#L46-L156
[S-composite-build]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_build_c.py#L203-L435
[S-config]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/config.env.example#L65-L304
[S-config-audio]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext11.py#L249-L306
[S-config-misc]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext00d.py#L134-L238
[S-config-video]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext08.py#L418-L492
[S-constants]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/core/constants.py#L32-L470
[S-copy-cut]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext07.py#L140-L236
[S-dispositions]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/metadata.py#L195-L267
[S-encoder-options]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_base.py#L120-L158
[S-entry]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/FFmWiz.py#L360-L713
[S-env-parser]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/L00_misc_b.py#L244-L265
[S-extract]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/modes_b.py#L99-L162
[S-extract-jobs]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext01b.py#L449-L495
[S-ffmpeg-nvenc]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavcodec/nvenc.c#L805-L852
[S-ffmpeg-nvenc-h264]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavcodec/nvenc_h264.c#L110-L125
[S-ffmpeg-nvenc-hevc]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavcodec/nvenc_hevc.c#L105-L120
[S-ffmpeg-tonemap]: https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L24200-L24212
[S-flow]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_flow.py#L105-L348
[S-folder]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/modes.py#L451-L585
[S-hardsub-audio-policy]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext00d.py#L432-L476
[S-hardsub-build]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_build_subtitles.py#L101-L260
[S-hardsub-options]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext01d.py#L101-L349
[S-join]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/modes_join.py#L244-L430
[S-join-audio]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext00_join_audio.py#L1-L200
[S-join-policy]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext01d.py#L361-L471
[S-look-constants]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/core/constants_look.py#L15-L61
[S-look-parser]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/L01_filters.py#L79-L107
[S-loudnorm]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/trackmanager.py#L270-L428
[S-metadata]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/metadata.py#L141-L553
[S-metrics]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext04.py#L282-L342
[S-nvidia]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext00b.py#L233-L277
[S-quality]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext01c.py#L274-L321
[S-quick]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_quick.py#L63-L127
[S-quick-build]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_build_c.py#L448-L577
[S-raw]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_raw.py#L35-L278
[S-report]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/modes_mediainfo.py#L114-L577
[S-report-csv]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext01.py#L312-L456
[S-report-scan]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext09.py#L196-L254
[S-reverse]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/reverse_pipeline.py#L117-L706
[S-streams]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/L01_streams.py#L75-L240
[S-track-builder]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext01b.py#L332-L432
[S-track-manager]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/trackmanager.py#L516-L783
[S-transform]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/modes_transform.py#L99-L200
[S-transform-build]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/support/ext04b.py#L85-L387
[S-wizard-steps]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/wizard_steps.py#L96-L778
[S-capability-identity]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/services.py#L137-L181
[S-timeline]: https://github.com/KiaroSama/FFmWiz/blob/661a0d240ee29ca7cda496b635ae8c50e43d916b/ffmwiz/core/timeline.py#L44-L215


