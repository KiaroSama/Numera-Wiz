# Numera Wiz v7: selected missing media contracts

Integration-only amendment to the v6 mandate. All affected capabilities remain **NOT_IMPLEMENTED / NOT_EXECUTED / NOT_QUALIFIED**. Retain the complete existing functional baseline and canonical capability IDs. This file adds three narrow missing contracts; it is not a replacement architecture or a second implementation directive. Evidence and affected main/input anchors are in `media-feasibility-review.md`.

## 1. Temporal effects sample a defined graph prefix

Insert after **Video graph order, effect clocks and nested composition**.

A temporal image node samples its immediate upstream graph at other times. Its input includes all preceding authored processing, including earlier temporal nodes, and excludes itself and all downstream processing. Use a finite acyclic dependency graph; reject a self/downstream cycle. Raw-source analysis is a separate declared sampling target. This selected Numera behavior intentionally does not inherit FilmCraft's skip-prior-temporal implementation limitation.

Source basis: [FilmCraft's distinct processed-layer and raw-source reads](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/mod.rs#L38-L121) and [EffectCraft temporal inputs](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/time_fx.rs). The Numera sampling/clock rules below are explicit selected semantics.

Each temporal descriptor records `sample_target`, `sample_clock`, sampling/quantization rule, boundary policy, history/lookahead, output extent and an admitted sample-schedule strategy. Parameter evaluation clocks remain independent: a source-anchored curve may compute an echo delay whose unit is a clip-clock second. Preserve that distinction through clip speed, reverse, holds, nesting and split/rebase operations.

For the initial clip Echo, select `sample_target=upstream_prefix` and `sample_clock=clip`. At output clip time `t`, the current input is tap zero and tap `k` requests the upstream result at `t + k * delay(t)`. The existing maximum of eight echoes means at most eight additional taps plus the current sample. Negative delay looks backward and positive delay looks forward. Apply the selected composition in its declared color/alpha domain. The default endpoint is transparent outside the upstream clip extent; an explicit edge-hold policy is a distinct choice. Echo does not change clip duration or audio; extensions use the existing explicit finite-tail contract. State the normalization of the selected average mode in the descriptor rather than inheriting incompatible donor formulas.

For the initial hold-rate effect, select clip-relative quantization with rational rate `r` and phase `p`: `q(t) = p + floor((t-p)*r)/r` for constant admitted `r > 0`. Sample the upstream prefix at `q(t)` while retaining output cadence and clip duration. A separately requested nearest rule must be named. Upstream animation is evaluated at the sampled time; downstream animation is evaluated at the current output time under its own declared parameter anchor. This operation is distinct from holding the decoded source while allowing all following effects to keep advancing.

Static schedules use shared upstream evaluation plus bounded split/trim/setpts/hold branches where admitted. Nonconstant delay, hold rate or phase is a sample-scheduling problem, not merely a runtime blend coefficient. Compile the requested output-tick-to-input-time map, preserving declared parameter interpolation. Select monotonic sections, explicit holds/reverse sections, or bounded materialized upstream regions. Share identical dependency windows and count all simultaneous state/history/lookahead. Reject an unadmitted schedule before execution with a concrete bound and explicit alternative; do not silently freeze an animated parameter, change sampling, open one decoder per frame, or build a whole-duration per-frame graph.

A nested temporal effect reuses its own upstream rendered dependency where useful. Materialization includes source/graph revision, all relevant parameter anchors, sample schedule, precision and random state in its identity. A bounded render graph cannot be obtained by bypassing expensive upstream effects or resetting their state.

### Required mapping edits

| Input row / field | Replacement meaning |
|---|---|
| FilmCraft `echo` / `ffmpeg_route` | Sample bounded windows of the immediate upstream processed layer in clip time; lower shared prefix output to aligned finite branches, or an admitted staged schedule, before weighted blend/overlay. Raw-source taps are not the default. |
| FilmCraft `echo` / `target_schema_boundary` | Signed clip-time delay; additional tap count; starting intensity/decay; named composition including explicit average normalization; sample mode; endpoint/tail policy; admitted animation schedule. Remove the generic speed/reverse/dependent-audio suffix. |
| FilmCraft `echo` / `parameter_or_algorithm_difference` | Finite acyclic upstream-prefix evaluation, including earlier temporal nodes; explicit schedule and endpoint limits. Record the deliberate difference from FilmCraft's skipped earlier temporal nodes. |
| FilmCraft `posterize_time` / observed and target definition | Hold the preceding processed layer at quantized clip-relative sample times while preserving output cadence and duration. Correct the observed description that currently says only source samples. |
| FilmCraft `posterize_time` / schema and route | Rational rate and phase; clip clock; floor/explicit nearest rule; endpoints and animation schedule; upstream-prefix hold lowering. Remove inherited speed-curve, reverse and audio-retime controls. |
| EffectCraft `ec.time.echo` / route and input qualification | Bounded upstream-layer branches at explicit layer/clip sample times, with handles/EOF, prior effects and parameter-clock evaluation retained. Replace the unqualified source-time claim. |
| Cross-donor `nw.video.time.echo` alias | Shared processed-layer echo; signed **clip-clock** offsets, count, sample mode and finite endpoints. Source-clock behavior, if added, is a named variant with explicit inversion/hold/reverse semantics. |
| Cross-donor `nw.video.time.hold_rate` alias | Shared upstream-layer quantization in the declared clip clock. Avoid the ambiguous combined source/effect-clock label. |

These edits refine semantics and remove unrelated controls; they do not remove either capability, change donor row counts, or assert donor mathematical identity.

## 2. Input format changes cannot silently reset an exact render

Insert after **Probe, indexing and incomplete sources**, with a cross-reference from **Finite tails, partitioning and lightweight execution**.

Represent observed format epochs for each decoded input: time interval, dimensions/pixel format, sample format/rate/layout, and the color/alpha/orientation metadata used by the selected processing contract. Record observation coverage and confidence. A bounded initial probe cannot prove all later frames have the same properties. Do not conflate source identity, declared codec parameters and observed frame-format stability.

The pinned manual describes state loss on reinitialization, and the implementation checks additional frame metadata/context changes. Its hardware-context check follows ordinary reinitialization suppression. [Pinned manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/ffmpeg.texi#L1366-L1394) · [Pinned trigger and diagnostic](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/fftools/ffmpeg_filter.c#L3064-L3215)

The default exact filtered-output policy is `reject_unexpected_change`. A declared stable authored stage cannot accept a native graph reconfiguration as continuous execution. For the bundled route select `-reinit_filter 1` and `-drop_changed 0` explicitly at the correct input/stream option position. Keep info-level graph diagnostics available independently of the user's retained-log display threshold. Neither `-reinit_filter 0` nor a changed-frame-dropping workaround is a correctness proof. Hardware-context changes belong to stage/device admission even when ordinary frame-property reinitialization is disabled.

For the selected bundled CLI, stream-observe the pinned `Reconfiguring filter graph` diagnostic at an admitted info log level and classify any unplanned post-initialization event as `INPUT_FORMAT_CHANGED` or the more specific device-context failure. Match the qualified filtergraph log context/message, not a substring inside arbitrary media metadata. The observer runs before log retention/rate caps. Abort/invalidate the affected stage, leave no verified/published result from that attempt, and retain the source/time/property evidence available. Pin and regression-test the observer with the executable profile; an external profile whose observation cannot be qualified is not eligible for a claimed continuous exact route. This diagnostic covers the implementation's trigger set, not every possible metadata change. Additional characterization that the requested operation relies upon needs its own frame-index/observation coverage.

Offer `normalize_epochs` only as a separately admitted route. Build a sufficient decoded epoch index, select explicit geometry/color/channel interpretation at boundaries, and prepare stable-format segments before the authored stateful processing. Preserve a common presentation/sample map and account for decoder preroll, resampler boundary samples, finite tails and intermediate precision. Feed the normalized stream to a continuous downstream graph; do not cross an unplanned native reset and call the result normalized. A qualified route may use bounded disk staging. If normalization is not admitted for the source/profile, report the exact unsupported boundary without discarding the rest of the application's capability baseline.

Retain a direct path for ordinary stable media. Request full scans only when the selected operation needs complete epoch information, and account for their work in the plan. Pure stream copy stays governed by its separate packet/parameter/container eligibility contract; do not force it through this decoded-pixel path. Live capture may select a declared discontinuity/drop policy, but that output cannot masquerade as an exact offline render.

## 3. Preview has an explicit presentation target

Insert within **Native and browser preview**, after the transport profiles; cross-reference **Color, alpha and text**.

The initial common preview target is **SDR sRGB presentation** with an explicit media-to-display view transform. Store a versioned `PreviewDisplayContract` in each preview session/cache identity: source/working characterization, view transform, target transfer/primaries/range, alpha-inspection/background mode, transport color signaling and texture-upload interpretation. Keep it separate from the export profile. SDR audition of HDR media is labeled; it is not HDR export qualification or a claim about an arbitrary monitor's calibration.

Use the selected [egui image-upload semantics](https://docs.rs/egui/0.36.2/egui/struct.ColorImage.html) and [WebGL pixel-storage/conversion contract](https://registry.khronos.org/webgl/specs/latest/1.0/#PIXEL_STORAGE_PARAMETERS) as adapter inputs; actual supported-browser behavior still requires qualification.

FFmpeg performs the selected media/HDR view transform and any preview-only alpha-background composition. For the standard shared audition view, resolve transparency onto the same selected background/checkerboard in the engine and deliver opaque presentation pixels to both adapters. Keep the alpha/HDR export branch unchanged. A separate native raw-alpha mode must specify its own correct presentation/compositing behavior rather than using the standard opaque browser path as evidence.

Native packed RGBA bytes are full-range, sRGB-encoded color with explicitly declared alpha. The default opaque view has alpha 255. If a straight-alpha view is admitted, use the matching upload conversion exactly once; do not reinterpret linear, BT.709-transfer or already-premultiplied bytes as interchangeable `ColorImage` input. Texture/surface interpretation belongs to the versioned renderer profile.

The default browser H.264 audition profile declares its admitted pixel format, chroma siting, range, BT.709 primaries/transfer/matrix signaling and opaque alpha behavior. The browser presentation adapter must select and qualify conversion from that decoded video into the common sRGB canvas/texture interpretation. Record actual WebGL upload conversion/premultiplication state and available canvas color-space controls for the real supported browser. A newer specification attribute is not proof of implementation. Disabling browser conversion is not a universal fix; double conversion is also incorrect. Keep this work within the standard presentation APIs and the FFmpeg-produced view; do not add donor shaders or another media-effects engine.

Qualify the native and browser presentation stages against the same reference view with tolerances appropriate to the explicitly lossy/chroma-subsampled audition route. Distinguish texture/canvas value checks from OS/monitor capture. Inspect neutral ramps and near-black values for systematic transfer/range faults, color patches for matrix mistakes, and transparent-edge fixtures for composition errors. UI-layout parity, matching timeline timestamps and successful MSE playback remain separate necessary checks. Do not promise raw-native/encoded-browser pixel identity.

## Focused implementation acceptance additions

These are future **GitHub Actions** checks except the existing owner-only hardware-media qualification. They do not authorize local builds or local media/browser tests during mandate preparation.

| ID | Fixture and assertion | Existing owner/gate |
|---|---|---|
| MF7-J01 | Labeled moving frames with animated color before Echo, another effect after Echo, and 2× clip speed. Prove taps use the correct processed prefix and clip offset; changing effect order changes the intended result. | WP05 timeline compiler; WP09 canonical effects |
| MF7-J02 | Two temporal nodes plus hold-rate phase and an animated tap schedule containing hold/reverse boundaries. Verify independent expected sample IDs, output frame count, endpoint policy and bounded active dependencies. | WP05/WP09; existing resource admission |
| MF7-J03 | A source whose size/pixel format changes and another whose audio rate/layout changes inside a stateful render. The default exact route fails before publication; a separately qualified normalization route preserves counts, timing and downstream state. | WP05/WP08; stream/partition contracts |
| MF7-J04 | Matrix/range/alpha reconfiguration and saturated log retention. An unplanned reset remains observable and cannot produce a verified success. Hardware-frame-context variants run only in the established owner GPU harness when that path is qualified. | WP05; logging; owner GPU gate |
| MF7-J05 | Shared SDR reference patches, HDR-to-SDR samples and transparent-edge backgrounds through native RGBA and actual supported browser audition. Catch systematic transfer/range/alpha faults separately from admitted codec error. | WP05 preview; browser/native release matrix |
