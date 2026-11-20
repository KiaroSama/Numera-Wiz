# Numera Wiz v7: media feasibility review

Status: **source/document review only; NOT_IMPLEMENTED / NOT_EXECUTED / NOT_QUALIFIED**. This review does not change the 610 source-effect rows, 481 canonical capability baseline, 942 command rows, implementation scope, public availability, or CI-first execution policy. No application build, FFmpeg invocation, media test, browser test, or GPU test was performed.

This is a bounded follow-up to v6. It does not reopen the already selected timeline interval, parameter-anchor, finite-tail, partitioning, GPU admission, output verification, or portable-package contracts. It identifies three remaining concrete seams where those contracts still permit materially different media results. The companion `media-feasibility-contract.md` supplies the selected insertions and exact mapping edits.

## Findings at a glance

| ID | Classification | Consequence | Required change |
|---|---|---|---|
| MF7-01 | Confirmed mapping/provenance defect and missing temporal-input contract | An echo can sample the raw picture instead of preceding effects, or change its apparent spacing after a speed change. Posterize Time can hold the wrong stage. | Sample the preceding effect graph in an explicit node clock; correct the source-time alias and unrelated inherited schema suffixes. |
| MF7-02 | Design gap, with confirmed upstream reset behavior | A valid format-changing input can reset temporal state and discard queued frames inside a supposedly continuous render. Hardware context changes can still trigger a reset with ordinary reinitialization disabled. | Admit explicit input format epochs and reject unplanned graph reconfiguration; normalize qualified variable-format inputs outside the authored stateful graph. |
| MF7-03 | Design gap requiring browser/native runtime proof | Identical project graphs can produce different displayed transfer, range, black levels or alpha edges in native RGBA versus browser H.264 preview. | Select a shared SDR presentation target and explicit transport/upload interpretation; qualify color presentation separately from project-time and widget parity. |

## MF7-01 — Temporal taps currently name the wrong input and clock

### Exact affected anchors

- Main mandate: **6 / Video graph order, effect clocks and nested composition**, lines 1029–1037; Annex FilmCraft rows `posterize_time` and `echo`, lines 2834 and 2849; EffectCraft `ec.time.echo`, line 3378.
- Chapter input: `research_v6/mandate-media.md`, lines 79–87.
- Mapping inputs: `research_v6/filmcraft-functional-mapping-final.csv`, source IDs `posterize_time` and `echo`, physical lines 159 and 174; fields `source_function_observed`, `ffmpeg_route`, `target_schema_boundary`, and `parameter_or_algorithm_difference`.
- `research_v6/effectcraft-functional-mapping-final.csv`, `ec.time.echo`, physical line 277, `ffmpeg_route_design` and `inputs_and_component_qualification`.
- `research_v6/cross-donor-capability-aliases.csv`, `nw.video.time.echo`, physical line 266: `required_variant_or_parameter_migration` currently says signed **source-time** offsets. The hold-rate alias is physical line 268.

### Evidence and confirmed defect

FilmCraft's `FxEnv::frame` accepts an offset in timeline seconds, rebuilds the layer at that time, and applies preceding non-temporal effects. Its separate `source_frame` operation is the raw-source analysis route. Echo uses the former. Posterize Time quantizes clip-relative time and also requests the former. The donor deliberately skips preceding temporal effects; that limitation must not be falsely described as full recursive composition. [F1][F2]

EffectCraft's temporal host reads the layer with preceding effects. Its echo requests `ctx.time + i * echoTime`; Posterize Time quantizes layer time. Consequently the cross-donor statement that these are intrinsically source-time taps is not source-supported, even though Numera may intentionally introduce a separately named source-clock variant. [F3]

The FilmCraft mapping also appends generic speed-curve, reverse and dependent-audio controls to both Echo and Posterize Time. Those controls do not define these effects and obscure the actual sampling contract. The current generic FFmpeg route allows a compiler to branch raw input before an animated color/generator effect, even though its output should be echoed or held. A clip playing at 2× makes a 0.25-second clip-clock offset traverse 0.5 seconds of source; treating it as a source offset produces another result.

### Selected resolution

Use complete, finite, acyclic **upstream-prefix sampling** at the temporal node's stored stack position. The node excludes itself and all later nodes; earlier temporal nodes remain ordinary upstream dependencies. This intentionally removes FilmCraft's skip-prior-temporal limitation rather than pretending to reproduce it. Select clip-relative time for the initial clip Echo and hold-rate controls; keep the parameter's evaluation anchor distinct from the clock in which a requested tap is expressed. A source-clock alternative requires its own explicit time-map/inversion policy and is not implicit in the shared alias.

Static echo offsets can lower to shared upstream output plus bounded shifted branches. Animated offsets/rates are scheduling controls: ordinary `blend` gain expressions cannot implement arbitrary changing sample requests. Their lowering must explicitly handle monotonic sections, holds and reversals, or use bounded materialized upstream regions. Do not create an unbounded graph or silently make the affected controls static. The contract gives separate sample-schedule admission and exact row edits.

**Runtime proof still needed:** source-labeled frames plus an animated upstream effect, speed change, downstream effect and two temporal nodes; verify sample identities and order independently of FFmpeg's graph expressions. This is implementation work in CI, not evidence supplied by this review.

## MF7-02 — Automatic input reconfiguration can invalidate the continuous-state guarantee

### Exact affected anchors

- Main: **6 / Probe, indexing and incomplete sources**, lines 963 onward; **Correct edits, transitions and temporal processing**, lines 1057–1071; **Finite tails, partitioning and lightweight execution**, lines 1075–1085.
- Chapter input: `research_v6/mandate-media.md`, lines 13–21, 118–121 and 125–135.
- These sections cover incomplete observations, stateful history and deliberate partitions, but select no policy for an unplanned format boundary arriving inside one decoder/filter stage.

### Evidence and gap

The pinned FFmpeg manual enables filtergraph reinitialization by default and states that filter state and buffered frames can be lost. `drop_changed` replaces that behavior with dropped changed frames, which is also not an exact-render policy. The manual lists resolution/pixel format and audio format/rate/layout changes. [F4]

The pinned implementation has a wider trigger set: video matrix/range/alpha mode, display-matrix and downmix side data, and hardware-frame-context changes also participate. Its hardware-context test occurs **after** the code that clears ordinary reinitialization when `-reinit_filter 0` is selected. Therefore that option alone cannot implement a universal no-reset guarantee. Primaries/transfer metadata must not be inaccurately claimed to belong to this exact trigger list. [F5]

This is a gap in the mandate, not a demonstrated Numera runtime failure. It can affect ordinary stateful audio, temporal denoise, frame-counter-based animation or a GPU path whose frame context changes. Explicitly bounding queues and keeping one process alive does not preserve a filter's state across a native graph rebuild.

### Selected resolution

For exact filtered output, default to **reject unexpected format epochs**. Record whether stability was observed, fully indexed or remains unknown. Observe actual pinned-build graph reconfiguration independently of log retention; an unplanned event invalidates the stage and its unpublished outputs. Do not treat a successful FFmpeg exit or subsequent playback as recovery. A device/surface change requires an admitted new plan/generation, not a claim that the old state continued.

An explicitly selected `normalize_epochs` route may prepare each indexed source span into a stable declared format, then feed a continuous authored graph. It requires timeline/sample accounting at boundaries and precision-preserving intermediates; automatic in-place resets do not establish that contract. Ordinary stable media keeps the direct path. A full decoded scan is a planned requirement when a particular requested correctness contract needs complete epoch knowledge, not an invisible tax on every basic probe.

**Runtime proof still needed:** changed resolution/pixel format and audio rate/layout with a stateful filter; changed matrix/range/alpha metadata; qualified hardware-context cases in the owner GPU suite. The result must be a truthful failure or the qualified normalized output, never silent dropped frames or a reset labeled continuous processing.

## MF7-03 — Preview byte format does not yet define displayed color

### Exact affected anchors

- Main: **6 / Color, alpha and text**, lines 1087–1093; **Native and browser preview**, lines 1186–1220; native RGBA command at line 1199 and browser H.264/AAC candidate at line 1216; GUI preview parity at line 1373.
- Inputs: `research_v6/mandate-media.md`, lines 137–143 and 236–270; `research_v6/mandate-architecture.md`, lines 141–151.

### Evidence and gap

The `rgba` pixel-format label specifies channel storage, not the complete display transfer. The selected egui family distinguishes straight and premultiplied upload constructors; its compact color type expects encoded sRGB and premultiplies the encoded values. [F6]

For a DOM-video texture, WebGL has color-conversion and alpha-unpack state. Browser-default conversion can depend on the browser/source; disabling it does not turn Rec.709 RGB into sRGB. The cited current specification is a working draft, so it informs API review but does not prove any supported browser implements every new attribute. [F7]

V6 already correctly allows a lossy browser audition profile to differ from raw native preview. That does not account for a systematic wrong transfer, range expansion, double conversion or double premultiplication. No existing selected contract states which component owns the final view transform, what the native RGBA bytes mean, or how the encoded browser stream reaches the equivalent display target.

### Selected resolution

Select an explicit SDR sRGB presentation target for the initial common preview. FFmpeg performs the characterized media/HDR view conversion. The native descriptor carries full-range sRGB-encoded RGBA semantics and a single declared upload interpretation. The browser profile carries explicit YUV format/range/primaries/transfer/matrix signaling and a qualified path to the same sRGB canvas interpretation. Keep browser API conversion in the presentation adapter; do not introduce another media-effects engine.

Use a shared preview-only alpha-inspection/background composition in FFmpeg for the standard opaque audition view, making both transports display the same authored alpha result. The ordinary browser H.264 path does not preserve alpha. Raw native alpha presentation can remain a separately qualified mode. Neither background nor SDR view transform enters the alpha/HDR export graph.

**Runtime proof still needed:** neutral ramps, near-black patches, saturated colors, semi-transparent edges and known HDR view samples in native and actual supported browsers. Establish tolerance for encoding/chroma differences and inspect the presented texture/canvas separately from the media graph; a matching GUI screenshot layout is insufficient.

## Primary evidence consulted

- [F1] [FilmCraft temporal environment, pinned `vfx/mod.rs` lines 38–44 and 87–121](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/mod.rs#L38-L121). Local read: `research_v3/requirements-filmcraft-crates__render__src__vfx__mod.rs`.
- [F2] [FilmCraft Echo and Posterize Time, pinned `temporal.rs` lines 20–52](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/temporal.rs#L20-L52). Local read: `research_v3/requirements-filmcraft-crates__render__src__vfx__temporal.rs`.
- [F3] [EffectCraft temporal input semantics and implementations, pinned `time_fx.rs`](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/time_fx.rs#L1-L13), particularly lines 166–174, 264–272 and 323–342. Local read: `research_v2/sources/effectcraft/crates/effects/src/time_fx.rs`.
- [F4] [FFmpeg pinned CLI manual, reinitialization, changed-frame dropping and buffering limits](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L1366-L1394). Local read: `research_v2/ffmpeg-upstream/doc_ffmpeg.texi`.
- [F5] [FFmpeg pinned `send_frame` reconfiguration logic](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/fftools/ffmpeg_filter.c#L3064-L3215). Local read: `research_v3/ffmpeg-source/fftools_ffmpeg_filter.c`.
- [F6] [egui ecosystem `ecolor::Color32` 0.36.2](https://docs.rs/ecolor/0.36.2/ecolor/struct.Color32.html) and [egui `ColorImage` 0.36.2 upload constructors](https://docs.rs/egui/0.36.2/egui/struct.ColorImage.html). These are API evidence, not a mandate to change the project's selected dependency pin.
- [F7] [Khronos WebGL 1.0 editor's draft, texture upload and pixel storage parameters](https://registry.khronos.org/webgl/specs/latest/1.0/#PIXEL_STORAGE_PARAMETERS), retrieved 2026-10-21. The reviewed draft identifies itself as 2026-06-30; snapshot the exact revision during implementation research and qualify actual browser behavior.
