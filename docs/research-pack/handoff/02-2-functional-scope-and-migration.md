> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## 2. Functional scope and migration

### Product contract

Build **Numera Wiz** as a new application. Use FFmWiz as the behavioral source for its media utilities, both standalone MuxCls and its embedded FFmWiz derivative for the complete F08 cleanup/remux rewrite, FilmCraft as the source for the GUI/editing experience and shortcuts, and FilmCraft plus EffectCraft as feature-discovery sources for FFmpeg-supported effects. Own the Rust architecture, public schema, project format, presets, documentation and release process.

The initial release must include the complete FFmWiz workflow baseline, usable FilmCraft-style editing and all discovered donor feature families for which the pinned FFmpeg build has an admitted, practical implementation. Each donor feature must have a recorded disposition; “difficult” or “not a filter with the same name” is insufficient reason for omission. A small first implementation milestone is allowed; calling that milestone the complete initial release is not.

Functional equivalence means the user can perform the same meaningful task with defined Numera semantics. It does not mean reproducing every source DSP/raster algorithm, numerical default or private ID. Numera controls must describe the implemented FFmpeg processor truthfully. Preserve exact data/edit semantics where those are part of the task, including frame/sample timing, clip ranges, track routing, stream selection and undo. Preserve visual resemblance and interaction behavior in the shared GUI. Preserve donor numerical effect behavior only for a separately scoped compatibility feature with evidence.

The Rust application owns projects, edit decisions, descriptors, document operations, UI, validated command/control data, caching and orchestration. FFmpeg/ffprobe perform the admitted media decoding, processing, analysis, composition, encoding and muxing. Parameter interpolation, filtergraph generation, ASS/VGS documents, LUT values, edit lists and bounded control/IR descriptions are legitimate orchestration artifacts. They must not conceal a new general-purpose donor DSP/render engine.

### Required ledgers and status vocabulary

Create small, reviewable machine-readable ledgers and generate their Markdown views. Do not generate thousands of redundant headings or expose donor internal aliases as separate product promises.

#### Donor coverage ledger

Each row contains: source repository and revision; source file/definition; meaningful behavior; canonical Numera capability IDs; status; parameter differences; user-visible consequence; evidence; responsible implementation task.

Use these statuses:

| Status | Meaning and release consequence |
|---|---|
| `native` | A dedicated FFmpeg operation/filter implements the documented Numera function. Required initial-release implementation |
| `composed` | An explicit bounded graph of supported operations implements the documented function. Required after correctness and resource qualification |
| `component` | Requires a particular compiled FFmpeg dependency, model, font, IR or asset pack. Install/setup and unavailable-state behavior are part of the feature |
| `application` | Rust project/edit/UI/document logic with no independent media engine. Required when part of the selected donor baseline |
| `unsupported` | A precise semantic feature lacks an admitted practical route in this FFmpeg profile; provide evidence, related supported alternatives and a clear user-facing explanation |
| `investigate` | Temporary research state with an owner and closure task. It cannot count as implemented or be silently omitted at a release review |

Do not report a supported family as excluded merely because a donor algorithm differs. For example, native chorus can be supported while the source's exact modulation curve is not a compatibility target.

#### Capability implementation ledger

For each canonical operation record: schema/version; units/ranges/defaults; inputs/outputs; effects on project revision; preconditions; filter/argv lowering; required FFmpeg features; latency/tail/color/alpha/time policy; CPU and optional GPU paths; memory/scratch estimate; GUI/browser entry point; CLI and both MCP transports; examples; fixtures/oracles; implementation/CI status.

The same operation and schema must serve every interface. GUI gestures resolve explicit domain targets before invoking it. Headless clients must not rely on a hidden focused panel or selection. Client-only presentation actions may be represented as named session/profile operations; they do not justify a second media API.

#### Optional compatibility ledger

FilmCraft/EffectCraft native project import, exact legacy effect IDs, source preset identity and host-plugin compatibility are separate compatibility features. They are not prerequisites for the native Numera product unless the user later requests them. If implemented, report `identical`, `converted`, `unsupported` or `data-only` per item. Require explicit conversion reporting. Never replace an unrecognized effect with a silent no-op or claim a converted preset is numerically identical.

Standard interchange workflows actually present in the selected editing baseline remain feature rows. Separating optional donor-native compatibility is not permission to remove EDL/XML/OTIO/AAF/OMF workflows that the final baseline explicitly includes. Each such format needs its actual supported subset and fixture evidence.

### Preserve all FFmWiz workflows through one engine

Annex A is the complete detailed behavioral reference. Its fifteen families below are required; canonical names may be harmonized without deleting capabilities. The source is [FFmWiz at the inspected revision](https://github.com/KiaroSama/FFmWiz/tree/661a0d240ee29ca7cda496b635ae8c50e43d916b).

| Family | Complete user task to preserve | Shared lowering / essential correction |
|---|---|---|
| F01 Encode/edit | Codec/container/rate control; crop/resize/rotate/flip; cuts/speed/reverse; color/denoise/sharpen/blur/fade; selected audio/loudness; stream extras; overlay/PiP/stacks/music; GIF/boomerang/thumbnail/loop | One timeline/stream plan. Edited thumbnails and GIFs honor the same cuts/effects. Native presets use encoder-specific controls |
| F02 Configuration | Load FFmWiz configuration and run deterministically | Import every known configuration field into a versioned job, report obsolete GUI keys/unknown values, use the ordinary planner without interactive prompts |
| F03 Copy cuts/splits | Keep/remove ranges and split outputs with stream copy | Distinguish packet/keyframe boundaries from frame-accurate re-encoding; preserve declared stream/timestamp/chapter policies |
| F04 Folder jobs | Batch media processing and aggregate results | Resolve every file independently; bounded scheduler; output-tree exclusion; collision-safe names; truthful partial failure |
| F05 Add tracks | Append external audio/subtitles while retaining picture | Explicit maps and container admission; strict-copy mode cannot silently transcode |
| F06 Extract streams | File/folder extraction of selected video/audio/subtitles | Per-source absolute identities; independent output clauses and per-artifact completion |
| F07 Inspect/analyze | Media reports, screenshots, packets/frames, volume and valid reference metrics | ffprobe plus declared FFmpeg analysis; stream large scans to disk; distinguish measured/estimated/incomplete; validate metric alignment |
| F08 Clean/remux / MuxCls | Complete standalone/embedded MuxCls Rust rewrite: select/order audio/subtitles, patch tags, independent attachments/data/covers/chapters, no-op copy, remux and companion-file batches | Annex D gives complete module/default/behavior and source-defect coverage. Resolve all stream outcomes; metadata-only and ordering-only changes are real operations; no Python/PowerShell/robocopy runtime dependency or second engine |
| F09 Burn subtitles | Internal/external text subtitles, fonts, quality, audio and HDR policy | libass path; actual HDR conversion; independent audio-stream compatibility; RTL/Unicode fixtures |
| F10 Video retime | Piecewise speed/reverse and output splits | Rational timestamp mapping; bounded reverse staging; every retained dependent track has an explicit retiming policy |
| F11 Audio edit | Cut/speed/reverse with sample-rate/layout/cover policy | Accurate sample-domain graph or explicitly packet-based copy; bounded reverse; no arbitrary channel downmix |
| F12 Join | Ordered video/audio joins with copy/encode and CFR/VFR choices | Concat eligibility from real stream properties; preserve later-only tracks with declared silence/mapping; retime chapters/subtitles |
| F13 Metadata | Titles/languages/custom tags, dispositions, chapters, covers and color/SAR signaling | Output-relative maps; preserve unrelated disposition flags; signaling is distinct from pixel conversion |
| F14 Capabilities/cache | Inspect, refresh, clear and report actual processing support | Build/device fingerprints; advertised versus exercised capabilities; CPU-only must work without GPU probing |
| F15 Track manager | Remove/add/replace and independent normalization | Per-final-signal loudness measurement; per-file/per-stream results; metadata-only requests cannot disappear |

FFmWiz's command-line experience is a useful behavior source, not an existing machine API to wrap unchanged. Build ordinary composable CLI commands and schema-driven MCP operations, and keep a guided wizard as one client of those operations.

### FilmCraft GUI/editing migration, with concrete task ownership

Use the pinned [FilmCraft GUI](https://github.com/storytold/filmcraft/tree/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui) and [engine behavior](https://github.com/storytold/filmcraft/tree/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src) as the reference. Port/adapt useful Rust UI structures subject to license/attribution; replace direct donor engine/render/DSP calls with the Numera client interface. Do not fork separate native and browser layouts.

| Task | Source/design to inspect and adapt | Delivered user behavior and backend binding |
|---|---|---|
| UI-01 Shared shell | `ui-egui/src/dock.rs`, `theme.rs`, icons, menus, workspace preferences | FilmCraft-like dark editor, split/tab dock, panel focus, resize, floating windows where platform permits, saved workspace. Shared Rust UI builds for native/browser |
| UI-02 Asset workspace | Project/Media Browser/Libraries/Info/Metadata panels; engine project/browser/relink/proxy operations | Import, bins, searchable/sortable assets, thumbnails, source metadata, missing-media relink, proxy lifecycle and ingest; thumbnails/proxies from the local FFmpeg engine |
| UI-03 Monitors | Source/Program/Reference Monitor/Timecode panels and playback actions | Source In/Out, sequence preview, frame/time navigation, playback/loop/shuttle, quality, safe margins/guides and comparison. Explicit exact/draft preview state |
| UI-04 Timeline/tools | Timeline panel, `clip_ops`, `sequence_tools`, keyboard actions | Tracks, targeted insert/overwrite, split/ripple/roll/slip/slide, lift/extract, snapping, linked A/V, locks, gaps, markers, nesting, undo/redo. Export compiles these edits through FFmpeg |
| UI-05 Effects | Effects and Effect Controls panels; project effect/transition definitions | Searchable native capability families, presets, ordered stack, enable/bypass, masks, keyframes and native controls generated from descriptors. No disabled donor clone catalogue presented as completed functionality |
| UI-06 Color/scopes | Color and Scopes panels | Working/output color, exposure/curves/LUT workflows and FFmpeg-generated scopes with correct color/time provenance; unsupported automatic matching is explicit |
| UI-07 Audio | Clip/Track Mixer, meters, Essential Sound; engine mixer/voiceover | Routing, faders/pan, mute/solo, inserts/sends, automation and capture. Same FFmpeg graph drives audible preview and export; native controls and qualified live updates |
| UI-08 Graphics/text | Essential Graphics, Properties, Text, TTS and caption/template operations | Text/caption edits, supported shape/graphic layers, styles, masks, templates and fonts; rendering/synthesis admitted through FFmpeg; unavailable optional components described clearly |
| UI-09 Multicam/project tools | Multicam, project manager, scene detection, interchange, transcript/remix operations | Data/edit workflows implemented in Rust; preview/analysis/render routes explicitly admitted. Do not infer tracking, beat analysis or diarization from unrelated filters |
| UI-10 Jobs/utilities | Progress/Events/History, export tools and added FFmWiz task views | Direct conversion/remux/track/metadata/batch workflows without a forced timeline project; job queue, cancel/retry, final command inspector and logs |
| UI-11 Shortcuts | Engine shortcut resolver, UI keyboard dispatch, dock panel keys | FilmCraft Default effective bindings and focus behavior; platform modifiers; edit/remap/conflict UI. Browser-reserved shortcuts get reachable alternatives without changing the domain operation |

Retain the source workspace arrangements: Editing, Assembly, Color, Effects, Audio, Captions and Graphics, Learning, Review, All Panels. Adapt product/help destinations to Numera. Keep panel roles even where marketing-specific names should become generic Numera labels. Major panel shortcuts in the pinned dock are Shift+1 Project, Shift+2 Source, Shift+3 Timeline, Shift+4 Program, Shift+5 Effect Controls, Shift+6 Track Mixer, Shift+7 Effects, Shift+8 Media Browser and Shift+9 Clip Mixer. Other bindings must come from the assembled source resolver, not memory or a presumed Premiere shortcut table. [Dock source](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/ui-egui/src/dock.rs) · [Resolver source](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/engine/src/shortcuts.rs)

Each UI task is complete only when the corresponding native and browser journey is usable, its operations are reachable through CLI/MCP, and the output is verified in CI. A screenshot of a populated panel alone is not feature completion.

### Practical effect migration inventory

The routes below identify native starting implementations, not finished graphs or measured performance. Resolve exact options against the pinned bundled FFmpeg build. Qualify color/alpha/channel/time domains, latency, tails and runtime commands before enabling a route. Effects with different algorithms use Numera-native controls and presets; donor names are feature provenance.

#### Picture and transitions

| User-facing family from the donors | Preferred native route | Definition and admission requirements |
|---|---|---|
| Crop/scale/fit/fill/rotate/flip/position | `crop`, `scale`/qualified hardware scale, `pad`, `transpose`, `rotate`, `hflip`, `vflip`, `overlay` | Explicit geometry/SAR/chroma alignment and transform order; animated values only where the route supports them |
| Brightness/contrast/gamma/saturation/hue | `eq`, `hue`, `colorbalance`, `colorchannelmixer`, `curves`, `lutrgb`/`lutyuv` as appropriate | Native units, working color space, clamping and bit depth. A donor slider value is not copied onto a different filter without conversion |
| Color transforms, LUTs and HDR | `colorspace`, `zscale`, `lut1d`, `lut3d`, admitted `tonemap`/hardware route | Source/working/output characterization; linear-light tone mapping where required; explicit metadata policy |
| Box/Gaussian/average blur | `boxblur`, `gblur`, `avgblur` | Native radii/sigma and planes; alpha treatment declared. No donor three-box identity requirement [V1] |
| Directional/motion/bokeh-like blur | Admitted `dblur`, temporal mixing or supported convolution/map composition | These are different functions. Do not label temporal blending as optical motion blur or claim arbitrary donor aperture reconstruction |
| Sharpen/edges/emboss | `unsharp`, `cas` where available, `convolution`, `edgedetect`, `sobel`/`prewitt` | Named kernels/strength; output clipping/alpha policy. Prefer dedicated bounded filters over pixel expression loops |
| Noise/grain, posterize/threshold, pixelation | `noise`; LUT/threshold composition; downscale/upscale or `pixelize` when available | Versioned noise seeds and visual definitions; no claim to source RNG identity |
| Video noise reduction/deband | `hqdn3d`, `atadenoise`, `fftdnoiz`, `nlmeans`, `bm3d`, `deband` as distinct processors | Low-cost default and explicit higher-cost profiles; temporal buffers/search settings in budget |
| Chroma/luma key, matte and blend | `colorkey`/`chromakey`, `lumakey`, `despill`, `alphaextract`, `alphamerge`, `maskedmerge`, `blend`, `overlay` | Key color domain, premultiplication and blend semantics; unsupported spill models are not silently approximated |
| Masks, shape fills/outlines, text | Supported `drawvg`, `drawbox`, `drawtext`, libass and mask composition | Shape/keyframe data in Rust; rasterization in FFmpeg. Validate fonts, shaping, feather/expansion and coordinate mapping [V2] |
| Perspective, lens/distortion/displacement | `perspective`, `lenscorrection`, `v360`, `displace`, `remap` for the functions each actually implements | A coordinate map is admitted only with bounded generation/cache cost; lens correction does not imply general 3D camera solving |
| Fade/dissolve/wipe/slide/zoom transitions | Native `xfade` modes plus supported `blend`/mask compositions | Normalize both input timing/format requirements; correct handles, duration, A/V crossover; donor private transition math is not the target |
| Retiming, frame interpolation, freeze, reverse | `setpts`, `fps`, `minterpolate`, `tpad`/single-frame hold, bounded `reverse` plans | Separate duplication/blending/motion interpolation modes and costs; no morph-cut identity claim [V3] |
| Stabilization | Two-pass `vidstabdetect`/`vidstabtransform`; separately named `deshake` route | Analysis sidecar, crop/zoom/smoothing controls, build dependency and preflight. Not general object tracking [V4] |
| Scopes, scene analysis, comparisons | `waveform`, `vectorscope`, `histogram`, `signalstats`, `scdet`, `psnr`, `ssim`, optional `libvmaf` | Exact source/time/color identity; metrics require aligned inputs; expose only supported numeric analysis fields |
| Stylized combinations/glows/shadows/look presets | Versioned short graphs from supported blur/color/alpha/blend/geometry primitives | A preset is an ordered graph with actual native controls, not a separate copied renderer |
| Procedural 3D lights/particles/ray tracing, advanced tracking/inpainting | No blanket admission | Inspect each feature. Admit only a specific available FFmpeg route with honest semantics and resource evidence; otherwise record the precise unsupported feature |

#### Sound

| User-facing family from the donors | Preferred native route | Definition and admission requirements |
|---|---|---|
| Gain, fades, channel maps, pan and mixing | `volume`, `afade`, `acrossfade`, `pan`, `channelmap`, `amerge`, `amix` | Gain law, routing, normalization, channel labels, clipping, duration and tail behavior explicit |
| EQ/high/low/band filters | `equalizer`, `bass`/`treble`, `highpass`, `lowpass`, `bandpass`, `bandreject`, `biquad`; `firequalizer`/`afireqsrc`+`afir` for declared FIR designs | Hz/dB/Q or native width controls, filter order and phase policy; no arbitrary source coefficient reconstruction requirement |
| Compressor/gate/expander/ducking | `acompressor`, `agate`, `sidechaincompress`, `sidechaingate`, `adrc` where spectral processing is intended | Detector/threshold/ratio/knee/attack/release/channel-linking defined by selected native effect; no sample-by-sample donor recurrence by default |
| Limiter/soft clipping/mastering | `alimiter`, `asoftclip`, admitted EQ/compressor/loudness chain | Set `level` deliberately: automatic output normalization must not defeat a chosen ceiling. Set `latency`/lookahead compensation explicitly. True-peak claims need an independent measured contract [A1] |
| Loudness/normalization | `loudnorm`, `ebur128`, `astats`, appropriate gain/dynamic normalization | Per-final-signal measurement; distinguish static gain, dynamic normalization and two-pass loudness |
| Alignment delay/echo/multitap | `adelay`, `aecho`, or bounded delayed branches/IR composition | Native delay units/tap gains; channel routing; finite tail. Recursive-feedback identity is not implied [A2] |
| Reverb/convolution | `afir` with licensed supplied/user IRs or admitted deterministic IR source | Partition size/latency, IR length, actual `irnorm`/`irgain`, wet/dry and channel layout. Deprecated no-op options (`gtype`, `response`, `channel`, `size`, `rate`) cannot implement a control. Room presets are Numera presets, not cloned FDN processors [A3] |
| Chorus/flanger/phaser | `chorus`, `flanger`, `aphaser` | Native controls and LFO/interpolation behavior; no source stage-count or feedback-law identity promise [A4] |
| Tremolo/vibrato/autopan/ring modulation | `tremolo`, `vibrato`, `apulsator`; bounded `amultiply`/source graph when required | Actual modulation law/range, channel phase and automation support |
| Pitch/time stretching | `rubberband` component; `atempo` for tempo; separately declared resampling-based pitch route | Independent pitch/tempo intent, formant/transient/quality controls and latency; do not substitute pitch-changing resampling for tempo-only intent [A5] |
| Click/noise removal and speech cleanup | `adeclick`, `adeclip`, `afftdn`, `anlmdn`, `deesser`, optional `arnndn` | Named native processors and parameters; models/data footprint explicit; no false dereverb/source-separation claim [A6] |
| Dialogue center enhancement | `dialoguenhance` when that specific function is requested | Its stereo input becomes 3.0 output; a stereo deliverable needs an explicit downmix plan. Do not present it as arbitrary same-layout speech cleanup |
| Stereo width/M-S/binaural/spatial | `stereotools`, `stereowiden`, `pan`, `headphone`, optional `sofalizer` | Distinguish simple widening from HRTF processing; channel order, IR/SOFA source/license and latency explicit |
| Distortion/excitation/bit effects | `asoftclip`, `acrusher`, `aexciter`, small bounded `aeval` waveshaping where needed | Native definitions and anti-aliasing limitations; do not promise analog circuit identity |
| Transcription/narration | Optional FFmpeg `whisper` and `flite` builds with approved models/voices | Text editing/import is independent; language/word timing/diarization/voice inventory and model cost must be truthful |
| Exact source FDN, nonlinear feedback, adaptive dereverb or proprietary host plugin | No automatic admission | Keep the supported ordinary family where applicable; mark the specific unimplemented semantics with evidence and a related native alternative |

#### Evidence for corrected native routes

- **[V1]** [gblur implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_gblur.c), [boxblur implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_boxblur.c).
- **[V2]** [drawvg implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_drawvg.c).
- **[V3]** [minterpolate implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_minterpolate.c).
- **[V4]** [vidstabdetect implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_vidstabdetect.c), [vidstabtransform implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_vidstabtransform.c).
- **[A1]** [alimiter implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_alimiter.c).
- **[A2]** [adelay implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_adelay.c), [aecho implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_aecho.c).
- **[A3]** [afir implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_afir.c).
- **[A4]** [chorus implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_chorus.c), [flanger implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_flanger.c), [aphaser implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_aphaser.c).
- **[A5]** [rubberband wrapper](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_rubberband.c).
- **[A6]** [adeclick implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_adeclick.c), [afftdn implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_afftdn.c).

The remaining route names are grounded in the pinned [FFmpeg filter manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi); the implementation agent must inspect the selected filter/option sections and build availability before finalizing any concrete schema. This table deliberately does not claim every listed native filter is already included, executed or qualified in a Numera package.

### Performance and acceptance rules

1. Prefer dedicated native filters, then short composed graphs, then bounded expressions only where necessary. Mathematical expressibility is not sufficient admission.
2. For a proposed default, estimate buffer storage and operation growth with dimensions, frame rate, channels, sample rate, radius, taps, history and graph branches. Reject accidental quadratic or delay-length-per-sample work for basic effects when a better admitted native route exists.
3. The plan records actual filters, transfers and resource estimates; CI measures real packaged artifacts. Owner hardware qualifies real GPU paths. Do not describe unexecuted recipes as fast or GPU-tested.
4. Preserve a CPU-only path for every core function. A function may be non-real-time at demanding settings; it must remain bounded, cancelable and honestly described. Optional heavy models are not loaded for ordinary editing.
5. A draft preview is explicitly marked and can reduce resolution or use a documented alternative. Final export executes the reviewed full-quality plan; a preview approximation never silently changes final effect semantics.
6. Missing optional components show installation/availability information and preserve authored data. Required bundled capabilities cannot be substituted with endless unavailable panels in the final release.
7. All build and normal qualification work runs in GitHub CI. Local owner GPU qualification uses the matching CI artifact. Source review and document linting are not a pretext to run application builds locally.


