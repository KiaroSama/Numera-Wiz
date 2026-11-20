# FilmCraft → Numera Wiz: final functional design mapping

**Source revision:** `d643a886650bfbf16f24762274054575aa1e8f20`. **FFmpeg revision:** `946fcce07b6dcd0331c8cc609192aeff5e1924f8`. **Date:** 2026-10-21.

This is a source-informed **design disposition for every one of the 303 FilmCraft effect/transition/intrinsic/graphic-layer inventory rows**. The complete machine-readable contracts are in `filmcraft-functional-mapping-final.csv`. It does not assert implementation or runtime qualification. Every row has `runtime_status=NOT_IMPLEMENTED_OR_EXECUTED`.

There are 52 native, 235 composed, 15 component-qualified and 1 precisely unsupported proposed design dispositions. No inventory row is unassigned or left as an `investigate` placeholder. Source semantics, parameter schema, selected lowering and resource assumptions must still be checked together during implementation; an assigned family or present filter name does not establish a complete route. Every implementation must pass its stated build/correctness/performance gates, and the optical-flow composition has a mandatory focused CI qualification task.

## Binding interpretation

The CSV's `target_capability_id` names a Numera effect, intrinsic property, graphic layer or transition capability, **not an action command**. Repeated target IDs intentionally merge source aliases that provide the same functional behavior. Distinct functions such as RGB color isolation versus hue isolation, crop-as-mask versus output-size crop, intrinsic gain versus inserted gain, and clock wipe versus corner-angle wipe remain distinct.

The source descriptors and defaults are provenance. New Numera controls use explicit native units and useful native semantics. Source-private rendering algorithms, RNG sequences, default values and native project/preset compatibility are not automatically part of the application API. This does not authorize silent effect removal. Every supported design still needs a working UI/control flow and equal CLI/MCP access; failures to implement a composition cannot be relabeled mathematical incompatibility without review.

`native` means a dedicated native route is selected; normal format/alpha/time adaptation may surround it. `composed` means a concrete bounded graph/parameter-generated vector or control artifact is selected. `component` identifies a real build/asset prerequisite such as Cairo, libvidstab, librubberband or HRTF data. `unsupported` identifies only the specified semantic function for which this review established no admitted practical route. It does not exclude a wider family.

All builds, ordinary media tests and performance qualification belong in GitHub CI. GPU qualification uses the owner's matching CI-built artifact. This review ran no application code, media jobs or builds.

## Findings that materially change the implementation

The bounded v7 temporal correction defines Echo and Posterize Time over the preceding processed layer in clip time, with parameter anchors kept separate from requested sample times. The source samples preceding non-temporal effects and skips earlier temporal nodes; Numera deliberately selects complete finite acyclic upstream-prefix sampling, excluding the current and downstream nodes. Animated offsets/rates require an admitted bounded sample schedule. The CSV removes unrelated inherited speed/reverse/audio controls from these two schemas. IDs, dispositions, counts and unqualified runtime status are unchanged. [Pinned temporal environment](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/mod.rs#L38-L121) · [Pinned temporal effects](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/temporal.rs#L20-L52)

The v5 media review corrected six inherited recipe contracts without changing inventory counts, capability IDs or dispositions. `color_emboss` now uses a finite spatial-relief lowering rather than a color-LUT template; `edge_glow` explicitly derives an image-edge mask; `alpha_glow` explicitly derives source alpha and places the original foreground over the glow. `four_color_gradient`, `ramp` and `gradient` use finite coordinate-dependent fields or matching native gradient modes; a color-only LUT cannot replace spatial generation. Shared EffectCraft descriptors use the same relief/field contracts. The CSV records focused fixtures for the defining spatial or alpha behavior, zero-strength identity, time/alpha/bounds and resource limits. These are corrected design requirements awaiting implementation and CI qualification, not validated media graphs. [Pinned spatial-expression manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15087-L15234) · [Pinned Sobel manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L22842-L22861) · [Pinned alpha extraction](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L8540-L8557)

The bounded followup inspected all 22 rows still assigned to the inherited color family and their 23 related EffectCraft source rows. All 22 FilmCraft schemas now retain their operation-specific controls without the unrelated exposure/gamma/curve suffix. `unsharp_mask`, `find_edges` and `emboss` now explicitly budget their spatial neighborhoods and intermediate planes; their difference fields no longer describe static pointwise color transforms. `strobe` now has an explicit clip-time gate, seconds/duty distinction, stable seek behavior and unchanged frame count/PTS. `invert` now selects actual RGB/alpha processing instead of allowing an RGB-only LUT to stand for alpha inversion. The four spatial/temporal operations have appropriate recipe families; counts, capability IDs and design dispositions remain unchanged. Shared EffectCraft contracts were reconciled, and unrelated optional gamma was removed from its brightness/contrast recipe. These are bounded schema/route repairs, not a new claim of full execution validation. [Native unsharp parameters](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L24520-L24605) · [Native component/alpha inversion](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L18666-L18694) · [Time and coordinate expressions](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15087-L15234)

The bounded light/noise followup reviewed 13 FilmCraft rows and corrected 11 routes plus four EffectCraft aliases. Turbulence now reaches an actual coordinate sampler; cell patterns compute nearest-site structure; signed vignette has actual darken/brighten interpolation; source-derived glow, streak and ray effects read source highlights; authored flare/leak fields remain distinct. Grain has explicit channel correlation, precision and overflow. EffectCraft's unclipped Noise wraps modulo one, while FilmCraft's preserves headroom; the canonical descriptor retains both named variants. These corrections do not change IDs, counts, dispositions or unqualified runtime status. The media contract defines common emitted-light alpha and bounded source/guide sampling; the per-row CSV contains the selected limits and focused oracles. See `research_v5/light-noise-review.md` and its field patch in the companion for the pinned source evidence.

1. **Crop is a mask/zoom effect in the source.** It must preserve canvas size unless zoom-to-fill/output geometry is explicitly selected. A bare native crop is not the complete feature.
2. **Enhance Speech is a conventional EQ/dynamics chain.** It does not require a speech model. The source high-pass/de-mud/presence/air plus expansion/compression task is retained using native processors.
3. **Auto Reframe follows a gradient-energy centroid.** A low-resolution FFmpeg analysis pass can emit scalar weighted measurements; Rust smooths those scalar controls and FFmpeg applies crop/scale. No learned detector is required or implied.
4. **Text Animator and Typewriter are image-cell transitions.** They do not need text recognition or a glyph-animation engine. Conversely, the actual structured text layer does need typography, run styles, vertical layout, background, shadow and multiple-stroke handling.
5. **Cube Spin and Flip Over are 2D source approximations.** They can remain efficient scale/swap/warp transitions. Basic 3D/Rotate 3D and most other card transitions are planar geometry, not a general scene renderer.
6. **The VR Spherical Blur transition is a flat approximation in the source.** It must not be used as evidence that a flat blur fulfills the stronger seam-safe `vr_blur` effect. Spherical effects and stereo eyes retain explicit seam/pole/layout policies.
7. **The source “Exponential Fade” uses a cubic curve.** Expose the actual chosen curve; blindly assigning native `acrossfade=exp` changes its meaning.
8. **Panner–Ambisonics is stereo virtual-source panning in the inspected source.** Do not invent B-format support from its display name.
9. **Mastering reverb remains usable.** Native IR convolution can implement the practical room-reverb section. The old restriction forcing its reverb control to zero is removed.
10. **DeReverb is the one unavailable semantic function in this mapping.** Its source late-reverberation predictor is not the same task as noise reduction or a gate. This review found no qualified stock FFmpeg equivalent; an actual future filter/component/model may reopen the row. All ordinary denoise, declick, de-ess, echo and reverb functions remain included.

Source evidence for these distinctions is attached per row; the inspected definitions include the [vfx catalogue](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/effect/vfx.rs), [audio catalogue](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/effect/audio.rs), [graphic layer schema](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/graphic.rs), and [transition implementations](https://github.com/storytold/filmcraft/tree/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/transitions).

## Shared implementation boundaries that avoid another impractical rewrite

### Native precision is checked per route

The pinned `perspective` implementation only advertises 8-bit picture formats. Its existence does not authorize silently quantizing the whole render. A bounded inverse-homography `geq` sampler is the specified float alternative; it performs a constant number of source lookups per output pixel. `remap` uses integer coordinate maps and cannot be treated as fractional precision by assertion. Alpha outside the plane needs explicit masks/guards because `geq` normally clamps out-of-range sampling. [Perspective source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_perspective.c) · [Geq source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_geq.c)

The pinned `convolve` route supports integer picture formats through 16-bit and normalizes its impulse by the coefficient sum. Aperture blur therefore has a concrete native route, with kernel alignment, padding and HDR range conversion exposed. The ordinary Gaussian family still uses `gblur`; it must not pay aperture-convolution costs. [Convolution source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_convolve.c) · [Gaussian source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_gblur.c)

The `varblur` route accepts a synchronized guide and supports GBR(P/A)F32. The guide must use the negotiated picture layout, with the scalar control replicated into each processed plane; the pinned common-format list has no GRAYF32. Its `min_r`/`max_r` labels are native controls, not a proven physical zero-to-R radius. Source `radiusf` derives from doubled native bounds and guide values, and `min_r=0` still has nonzero support. Specify source-derived guide calibration and exact original-image bypass for zero blur/focus, then qualify impulse response, edges and tiny dimensions in CI. Its summed-area tables alone allocate `nb_planes*(W+1)*(H+1)*(4 bytes if depth<=8 else 8)`, even for unprocessed planes. A single native processor can replace a large Gaussian stack while still requiring this material memory budget. [Variable blur formats and processing](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_varblur.c#L75-L245) · [Buffer allocation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_varblur.c#L344-L359)

### Procedural effects remain bounded

Native `perlin`/`noise`, small masks, LUTs, vector gradients and native blur cover ordinary procedural looks. A custom `geq` formula is justified for a small fixed coordinate field, selection mask or local gather. It is not a license to replicate all donor internal math. Distinguish O(1) coordinate sampling, fixed small gathers, and high-cost growing searches in the planner. Native morphology handles broad alpha outlines; large repeated-overlay or exact distance-transform constructions are not defaults. [Morphology source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_morpho.c)

Procedural seeds define repeatable Numera output. Optional blur samples, ray taps, slats or cells have explicit quality/resource parameters. A source-defined large setting is admitted according to the actual plan budget; it is not silently clamped or advertised as real-time. Interactive draft resolution is labeled, while final export follows the reviewed quality definition.

### Typography remains complete without a second pixel engine

The source graphic layer includes rich runs, alignment/justification, tracking, kerning/ligatures, vertical text, baseline/caps, boxes, gradient fill, multiple strokes, background, shadow and transforms. `drawtext` alone does not prove those controls work. Use native text/libass where it fits; for advanced typography, Rust can shape/layout fonts into **vector glyph paths** and FFmpeg `drawvg` rasterizes them. This is parameter/geometry preparation, not a Rust rasterizer. Cache glyph/layout/path artifacts; declare fonts and the standard Cairo-enabled component. The FFmpeg vector language is a path renderer, not an assumed rich-text shaping service. Its actual formats are RGB32, 0RGB32, RGB565 and X2RGB10; Cairo ARGB32 expects premultiplied alpha. Rasterize bounded coverage/graphic layers separately and compose them in the chosen working domain. A mask-first path can apply authored HDR colors after rasterization; declare coverage precision and alpha conversion. Do not send the whole float/HDR base picture through Cairo to add a title. [Graphic source](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/graphic.rs) · [Drawvg formats](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_drawvg.c#L2675-L2694)

### Audio delay and limiting use honest native definitions

Echoes have explicit finite tail/noise-floor semantics. A native circular delay, finite-tap `aecho` or partitioned `afir` plan replaces dense high-order `aiir`; colored/saturated echoes remain available without pretending the finite graph is an infinite nonlinear feedback loop. Do not force users to lose ordinary echo/reverb functions because the source algorithm differs. The concrete bound includes `afir`'s maximum supported IR duration. [Echo source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_aecho.c) · [FIR source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_afir.c)

Limiter output gain and latency are explicit. `alimiter` automatic normalization must not silently defeat a ceiling. A true-peak export mode is a measurable contract using declared oversampling/limiting/resampling and a final FFmpeg true-peak measurement. If it misses the ceiling, the declared policy rerenders with a margin or fails; an unchecked result is never called verified true-peak. [Limiter source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_alimiter.c)

## Motion morph: concrete design requiring a focused CI qualification

The FilmCraft source applies optical interpolation to its two transition images. FFmpeg `minterpolate` derives interpolation alpha from neighboring input presentation timestamps, rather than requiring a fixed 50% blend. This allows a one-process paired-stream design; it is a source-derived proposal, not a tested command.

For output frame time `t_n = n/F`, desired progress `p_n` in `[0,1]`, and `0 < delta < 1/(2F)`, assign the paired input frames:

```text
A_n.pts = t_n - p_n * delta
B_n.pts = t_n + (1 - p_n) * delta
```

Both per-input sequences stay monotonic for any bounded progress sequence, and each pair surrounds the desired output time. The timestamps encode `p_n` as the interpolation fraction. Rebase/add lead-in to keep the desired input domain; use a sufficiently precise time base, interleave the two streams by timestamp, and request output rate F from `minterpolate` with bilateral motion estimation and deliberate scene-cut policy. This produces O(N) input frames, avoiding N squared intermediate-frame generation or one process per output frame. The exact endpoints can bypass the morph stage.

The qualification task must prove frame selection, **multi-frame context and neighboring-pair estimation behavior**, internal alpha quantization, edge-frame padding/flush, EOF duration and scene-cut behavior using moving fixtures. The pinned route's format limitations must be explicit: ordinary resolved opaque 8-bit pictures are the initial native domain; high-depth or transparent-layer motion interpolation needs a separately qualified route and cannot be silently flattened/quantized. The design remains `NOT_IMPLEMENTED_OR_EXECUTED` until this gate passes. If the focused spike fails, withhold this specialized motion-morph variant, record the failed boundary and retain the ordinary supported transitions; do not start a new first-party optical-flow renderer. This spike is not a prerequisite for the first basic-editor milestone and is not a validated default. [Source interpolation and frame scheduling](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_minterpolate.c#L1074-L1208) · [FilmCraft transition dispatch](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/transitions/dissolve.rs)

## Row-level design inventory

Each row below has its full typed boundary, native route, source evidence, parameter differences and resource policy in the CSV. `N` = native; `C` = composed; `P` = component; `U` = precisely unsupported. Every row is still unimplemented/unexecuted. These are capabilities, not separate required command names.

| Source ID | Numera capability | Design | Useful target definition |
|---|---|---|---|
| `brightness_contrast` | `nw.video.color.brightness_contrast` | C | Adjust picture brightness and contrast in an explicitly selected signal domain. |
| `tint` | `nw.video.color.two_color_tint` | C | Map luminance between chosen dark/light colors and mix with the original. |
| `color_balance` | `nw.video.color.tonal_balance` | C | Apply separate RGB shadow, midtone and highlight corrections with optional luminosity preservation. |
| `leave_color` | `nw.video.color.hue_isolation` | C | Keep a selected hue neighborhood colored and desaturate other hues. |
| `change_to_color` | `nw.video.color.hue_replacement` | C | Shift hues selected by a circular hue key toward a target hue while retaining chosen luminance/saturation. |
| `lumetri` | `nw.video.color.grading_stack` | C | A sectioned color-grading stack with primary corrections, creative looks, curves, wheels, HSL secondary selection, LUTs, sharpening and vignette. |
| `gaussian_blur` | `nw.video.blur.gaussian` | N | Apply Gaussian blur with independent horizontal/vertical amount and declared edge treatment. |
| `directional_blur` | `nw.video.blur.directional` | N | Blur along an authored direction. |
| `sharpen` | `nw.video.sharpen.standard` | N | Sharpen using a native unsharp residual. |
| `unsharp_mask` | `nw.video.sharpen.thresholded` | C | Sharpen only residuals exceeding an authored threshold. |
| `camera_blur` | `nw.video.blur.defocus_simple` | N | Provide a simple image defocus blur. |
| `black_white` | `nw.video.color.monochrome` | C | Convert picture color to a declared luminance grayscale while retaining alpha. |
| `color_pass` | `nw.video.color.rgb_isolation` | C | Keep colors near an RGB key and desaturate the remainder, optionally reversing the selection. |
| `gamma_correction` | `nw.video.color.gamma` | C | Adjust signal gamma with explicit exponent convention. |
| `invert` | `nw.video.color.invert_channels` | C | Invert selected RGB channels or alpha and blend with the original. |
| `levels` | `nw.video.color.levels` | C | Remap input black/white and midpoint gamma into output black/white. |
| `proc_amp` | `nw.video.color.proc_amp` | C | Adjust brightness, contrast, hue and saturation as one convenient processor. |
| `extract` | `nw.video.color.luma_range_visualizer` | C | Replace RGB with a monochrome luma-range selection while preserving alpha. |
| `posterize` | `nw.video.color.posterize` | C | Quantize each color component to a chosen number of levels. |
| `mosaic` | `nw.video.pixelate` | C | Replace the image with a grid of reduced-resolution color blocks. |
| `find_edges` | `nw.video.edges` | C | Display an edge-strength image, optionally invert and mix with the source. |
| `emboss` | `nw.video.relief.emboss` | C | Create monochrome directional relief and blend with source. |
| `replicate` | `nw.video.tile.replicate` | C | Repeat the complete picture in a regular grid. |
| `strobe` | `nw.video.light.strobe` | C | Gate a color wash periodically in clip time. |
| `noise` | `nw.video.noise.grain` | C | Add mono or color grain with explicit scale, seed and clipping policy. |
| `median` | `nw.video.denoise.median` | N | Remove local impulsive image noise with a median neighborhood. |
| `crop` | `nw.video.mask.crop` | C | Cut away canvas edges with optional feather; optionally enlarge the retained rectangle back to the canvas. |
| `horizontal_flip` | `nw.video.geometry.flip_horizontal` | C | Reflect the picture horizontally. |
| `vertical_flip` | `nw.video.geometry.flip_vertical` | C | Reflect the picture vertically. |
| `edge_feather` | `nw.video.mask.edge_feather` | C | Feather opacity inward from the frame rectangle. |
| `transform` | `nw.video.geometry.transform` | C | Transform a layer using anchor, position, independent scale, rotation, skew and opacity. |
| `mirror` | `nw.video.geometry.mirror_line` | C | Reflect one side of an authored line to the other side. |
| `offset` | `nw.video.geometry.wrap_offset` | C | Wrap-shift the picture horizontally/vertically and mix with original. |
| `lens_distortion` | `nw.video.geometry.radial_lens` | C | Apply radial lens distortion with an adjustable optical center and optional original mix. |
| `twirl` | `nw.video.geometry.twirl` | C | Rotate sampling coordinates by a radial falloff around a center. |
| `wave_warp` | `nw.video.geometry.wave` | C | Displace the image with an animated directional sine wave. |
| `basic_3d` | `nw.video.geometry.planar_3d` | C | Tilt/swivel one planar layer with perspective and depth placement. |
| `drop_shadow` | `nw.video.shadow.drop` | C | Cast a colored blurred alpha shadow at an angle/distance, optionally show shadow only. |
| `bevel_alpha` | `nw.video.relief.alpha_bevel` | C | Shade the alpha boundary as a lit bevel. |
| `ultra_key` | `nw.video.key.advanced_chroma` | C | Key a background with matte generation, cleanup, spill reduction and output-preview modes. |
| `track_matte` | `nw.video.mask.track_matte` | C | Use another layer/track alpha or luminance as this layer coverage. |
| `color_key` | `nw.video.key.rgb` | C | Make colors near an RGB key transparent with feather and matte expansion/contraction. |
| `luma_key` | `nw.video.key.luma` | C | Derive transparency from a luminance threshold/range. |
| `four_color_gradient` | `nw.video.generate.four_color_field` | C | Blend four authored colors around defined positions into a smooth background field. |
| `ramp` | `nw.video.generate.two_color_ramp` | C | Generate a linear or radial two-color ramp between points and mix with the picture. |
| `circle` | `nw.video.generate.circle` | P | Draw a filled antialiased circle. |
| `grid` | `nw.video.generate.grid` | P | Draw a centered regular grid. |
| `lens_flare` | `nw.video.light.lens_flare` | C | Add a procedural light core, halo, streak and ghost elements along a lens axis. |
| `timecode` | `nw.video.text.timecode` | P | Burn rational-rate source or sequence timecode into the picture. |
| `clip_name` | `nw.video.text.clip_name` | P | Burn resolved clip name into the picture. |
| `simple_text` | `nw.video.text.burnin` | P | Burn text or resolved media/sequence metadata into the picture. |
| `motion` | `nw.clip.motion` | C | Intrinsic placement of source media in the sequence before effect/compositing stages. |
| `opacity` | `nw.clip.composite` | C | Set intrinsic clip opacity and blend mode against lower tracks. |
| `time_remap` | `nw.clip.time_map` | C | Map sequence time to clip source time with speed, reverse, holds and exact boundaries. |
| `volume` | `nw.clip.audio_gain` | N | Apply intrinsic clip gain before its declared downstream mix stage. |
| `channel_volume` | `nw.clip.channel_gain` | N | Apply intrinsic channel-specific gains without dropping unaddressed channels. |
| `panner` | `nw.clip.audio_pan` | N | Pan/balance clip audio according to declared source layout and pan law. |
| `constant_power` | `nw.audio.transition.equal_power` | N | Crossfade overlapping audio with an equal-power curve. |
| `constant_gain` | `nw.audio.transition.linear` | N | Crossfade overlapping audio with complementary linear gains. |
| `exponential_fade` | `nw.audio.transition.shaped` | N | Crossfade with a selectable nonlinear easing curve. |
| `amplify` | `nw.audio.gain` | N | Add a gain stage at the authored effect-stack position. |
| `channel_mixer_a` | `nw.audio.channel_matrix` | N | Mix stereo inputs through an editable matrix with output polarity inversion. |
| `channel_volume_a` | `nw.audio.channel_gain` | N | Apply per-channel gain at the authored effect-stack position. |
| `deesser` | `nw.audio.cleanup.deess` | N | Reduce excessive sibilance with native de-essing controls. |
| `dynamics_rack` | `nw.audio.dynamics.rack` | C | Provide independently enabled gate, expander, compressor, limiter, soft clip and output stages. |
| `dynamics` | `nw.audio.dynamics.compressor` | C | Compress level above threshold with optional soft knee and linked detection. |
| `hard_limiter` | `nw.audio.dynamics.limiter` | C | Limit output peaks with lookahead, and offer an export-measured true-peak ceiling mode. |
| `multiband_compressor` | `nw.audio.dynamics.multiband` | C | Split into four phase-consistent bands, compress independently or linked, then recombine. |
| `single_band_compressor` | `nw.audio.dynamics.compressor_hard_knee` | C | Apply hard-knee compression with explicit makeup gain. |
| `tube_compressor` | `nw.audio.dynamics.tube_style` | C | Combine RMS/soft-knee compression and gentle saturation for a tube-style character. |
| `analog_delay` | `nw.audio.delay.colored_echo` | C | Create colored, saturating repeated echoes with an explicit finite tail. |
| `delay` | `nw.audio.delay.echo` | C | Create repeated delayed echoes with editable decay and wet/dry balance. |
| `multitap_delay` | `nw.audio.delay.multitap` | C | Mix four independently timed, panned echo taps with individual decay and gain. |
| `bandpass` | `nw.audio.eq.bandpass` | N | Pass an authored frequency band. |
| `bass` | `nw.audio.eq.low_shelf` | N | Adjust low-frequency shelf gain. |
| `fft_filter` | `nw.audio.eq.response_curve` | N | Apply an arbitrary bounded frequency/gain curve using native FIR/spectral filtering. |
| `graphic_eq` | `nw.audio.eq.graphic_10` | N | Provide a ten-band graphic equalizer. |
| `graphic_eq_20` | `nw.audio.eq.graphic_20` | N | Provide a twenty-band graphic equalizer. |
| `graphic_eq_30` | `nw.audio.eq.graphic_30` | N | Provide a thirty-band graphic equalizer. |
| `highpass` | `nw.audio.eq.highpass` | N | Remove frequencies below cutoff. |
| `lowpass` | `nw.audio.eq.lowpass` | N | Remove frequencies above cutoff. |
| `notch` | `nw.audio.eq.notch_bank` | N | Apply a finite bank of adjustable frequency notches. |
| `parametric_eq` | `nw.audio.eq.parametric_full` | N | Provide high/low cuts, low/high shelves, five parametric bands and master gain. |
| `scientific_filter` | `nw.audio.eq.classical_iir` | N | Provide Bessel, Butterworth, Chebyshev and elliptic filter designs as stable low-order biquad sections. |
| `simple_notch` | `nw.audio.eq.notch` | N | Reject one narrow frequency band. |
| `simple_eq` | `nw.audio.eq.parametric_band` | N | Boost or cut one parametric band. |
| `treble` | `nw.audio.eq.high_shelf` | N | Adjust high-frequency shelf gain. |
| `chorus_flanger` | `nw.audio.modulation.chorus_flanger` | N | Select native chorus or flanger processing. |
| `flanger` | `nw.audio.modulation.flanger` | N | Apply a swept short delay with feedback and stereo phase. |
| `phaser` | `nw.audio.modulation.phaser` | N | Apply a native moving phase-comb effect. |
| `declicker` | `nw.audio.cleanup.declick` | N | Remove isolated impulsive clicks using native autoregressive repair. |
| `dehummer` | `nw.audio.cleanup.dehum` | N | Remove mains hum and selected harmonics with adjustable notches. |
| `denoise` | `nw.audio.cleanup.denoise` | N | Reduce broadband/spectral noise using a selected native processor. |
| `dereverb` | `nw.audio.cleanup.dereverb` | U | Late-reverberation suppression using a delayed spectral reverberation model. |
| `speech_enhance` | `nw.audio.preset.speech_enhance` | C | Enhance speech through high-pass, de-mud, presence/air EQ and expander/compressor stages. |
| `convolution_reverb` | `nw.audio.reverb.convolution` | C | Apply room/cabinet impulse-response convolution with native tone, predelay and wet/dry controls. |
| `studio_reverb` | `nw.audio.reverb.studio` | C | Provide a stereo room/reverb workflow with room-size/tail, damping and wet/dry presets. |
| `surround_reverb` | `nw.audio.reverb.surround` | C | Provide multichannel early/late room response with explicit speaker routing. |
| `binauralizer` | `nw.audio.spatial.binaural` | P | Render virtual loudspeaker positions to headphones using explicit HRTF/HRIR assets. |
| `distortion` | `nw.audio.distortion.waveshaper` | C | Apply selectable bounded soft/hard/asymmetric/sine shaping, tone filtering and dry/wet mix. |
| `fill_left` | `nw.audio.channels.right_to_both` | N | Copy right to left while retaining right. |
| `fill_right` | `nw.audio.channels.left_to_both` | N | Copy left to right while retaining left. |
| `guitar_suite` | `nw.audio.preset.guitar_suite` | C | Provide compressor, distortion, amp/cabinet voicing, final tone filter, mix and output sections. |
| `invert_a` | `nw.audio.polarity.invert` | N | Invert audio polarity. |
| `loudness_radar` | `nw.audio.analysis.loudness_meter` | N | Measure loudness and peaks at the selected insert position without changing signal. |
| `mastering` | `nw.audio.preset.mastering` | C | Provide EQ, reverb, excitation, stereo width, limiting/soft clipping and output gain in order. |
| `panner_ambisonics` | `nw.audio.spatial.virtual_stereo_pan` | N | Rotate two virtual stereo source directions and render their pan/elevation gains. |
| `swap_channels` | `nw.audio.channels.swap_stereo` | N | Swap left/right while preserving any additional channels. |
| `vocal_enhancer` | `nw.audio.preset.vocal_voicing` | C | Select voice or music voicings using EQ and gentle compression. |
| `stereo_expander` | `nw.audio.stereo.expand_pan` | N | Scale the side signal and pan the mid signal independently. |
| `stereo_width` | `nw.audio.stereo.width` | N | Adjust mid/side width. |
| `pitch_shifter` | `nw.audio.pitch.shift` | P | Shift pitch independently of duration with native quality/formant controls. |
| `balance_a` | `nw.audio.stereo.balance` | N | Balance left and right channels with a declared law. |
| `mute` | `nw.audio.mute` | N | Mute/unmute with a short de-click ramp. |
| `volume_a` | `nw.audio.gain_insert` | N | Apply bypassable gain at an inserted effect position. |
| `lighting_effects` | `nw.video.light.surface_lighting` | C | Shade a 2D image surface using up to five directional/point/spot lights, optional luminance bump and ambient/material controls. |
| `vr_blur` | `nw.video.vr.blur` | C | Blur a spherical image with seam-safe angular neighborhoods, per eye. |
| `vr_chromatic_aberrations` | `nw.video.vr.chromatic_aberration` | C | Displace RGB sample directions around a spherical center with adjustable falloff. |
| `vr_color_gradients` | `nw.video.vr.color_field` | C | Blend authored color points by spherical distance. |
| `vr_denoise` | `nw.video.vr.denoise` | C | Reduce spatial noise on a spherical frame while preserving strong edges. |
| `vr_digital_glitch` | `nw.video.vr.glitch` | C | Create wrapping band shifts, RGB offsets and scanline attenuation per eye. |
| `vr_fractal_noise` | `nw.video.vr.fractal_noise` | C | Generate a seam-safe evolving octave noise field on a sphere and composite it. |
| `vr_glow` | `nw.video.vr.glow` | C | Add thresholded highlight glow using seam-safe angular blur. |
| `vr_plane_to_sphere` | `nw.video.vr.plane_projection` | N | Project a planar picture into an equirectangular sphere with transparent uncovered areas. |
| `vr_projection` | `nw.video.vr.fov_projection` | N | Rotate an equirectangular view and apply an explicit field-of-view/crop/stretch policy. |
| `vr_rotate_sphere` | `nw.video.vr.rotate` | N | Rotate a full spherical picture without changing its declared projection/layout. |
| `vr_sharpen` | `nw.video.vr.sharpen` | C | Sharpen a spherical picture using a seam-safe blur residual. |
| `bokeh_blur` | `nw.video.blur.aperture` | C | Blur through a circular/polygon aperture, with optional bright-highlight enhancement. |
| `channel_blur` | `nw.video.blur.channels` | C | Blur R, G, B and alpha by independently chosen amounts. |
| `compound_blur` | `nw.video.blur.radius_map` | C | Vary blur radius using another layer or the current picture luminance. |
| `focus_blur` | `nw.video.blur.focus_region` | C | Keep a radial/linear region sharp and blur progressively outside it. |
| `reduce_interlace_flicker` | `nw.video.blur.vertical_soften` | N | Reduce line flicker through vertical-only softening. |
| `asc_cdl` | `nw.video.color.asc_cdl` | C | Apply slope, offset, power and saturation color correction. |
| `video_limiter` | `nw.video.color.signal_limiter` | C | Limit RGB and/or luma to declared output bounds, with soft compression and warning view. |
| `vignette` | `nw.video.light.vignette` | C | Apply a feathered elliptical/rounded light/dark border treatment. |
| `corner_pin` | `nw.video.geometry.corner_pin` | C | Map four source corners to an authored quadrilateral. |
| `magnify` | `nw.video.geometry.loupe` | C | Magnify a circular or rectangular region and composite it over the original. |
| `spherize` | `nw.video.geometry.spherize` | C | Magnify/warp the image inside a radial lens region. |
| `turbulent_displace` | `nw.video.geometry.turbulence` | C | Displace coordinates with an evolving multi-octave noise field. |
| `warp_stabilizer` | `nw.video.stabilize` | P | Analyze camera shake and smooth the picture transform in a two-pass stabilization workflow. |
| `gradient` | `nw.video.generate.gradient` | C | Generate linear/radial/reflected/diamond color ramps with midpoint, optional dither and blend. |
| `channel_mix` | `nw.video.color.channel_matrix` | C | Apply an affine RGB channel matrix with optional monochrome output. |
| `color_replace` | `nw.video.color.rgb_replacement` | C | Select an RGB-distance neighborhood and replace its color or selected hue/saturation components. |
| `rounded_crop` | `nw.video.mask.rounded_crop` | C | Mask to a rounded rectangle with feather and optional inner border. |
| `alpha_adjust` | `nw.video.alpha.adjust` | C | Change, replace, invert or inspect alpha while maintaining consistent color representation. |
| `logo_cutout` | `nw.video.key.flat_background` | C | Remove a known flat background and optionally un-composite its color from edge pixels. |
| `echo_glow` | `nw.video.light.echo_glow` | C | Combine progressively wider highlight glows with geometric decay. |
| `edge_glow` | `nw.video.light.edge_glow` | C | Turn image edges into colored blurred light. |
| `glint` | `nw.video.light.glint` | C | Create star-shaped streaks from bright image regions. |
| `light_leaks` | `nw.video.light.leaks` | C | Overlay animated colored light washes from seeded gradient blobs. |
| `rgb_split` | `nw.video.color.channel_offset` | C | Offset RGB channels linearly or radially around a center. |
| `volumetric_rays` | `nw.video.light.radial_rays` | C | Create finite radial highlight streaks from an authored point. |
| `wonder_glow` | `nw.video.light.multiscale_glow` | C | Combine several highlight blur radii with tint and saturation controls. |
| `long_shadow` | `nw.video.shadow.long` | C | Extrude visible alpha along a direction with optional fade. |
| `brush_strokes` | `nw.video.stylize.brush_strokes` | C | Create a painterly look using short directional gathers with per-cell variation. |
| `color_emboss` | `nw.video.relief.color_emboss` | C | Add directional relief while retaining source color. |
| `roughen_edges` | `nw.video.mask.roughen` | C | Perturb/erode alpha edges with procedural noise and selectable edge looks. |
| `posterize_time` | `nw.video.time.hold_rate` | C | Hold the preceding processed layer at quantized clip-relative sample times while preserving output cadence and clip duration. |
| `rotate_3d` | `nw.video.geometry.planar_xyz` | C | Rotate one planar layer around X/Y/Z with perspective and depth. |
| `auto_reframe` | `nw.video.analysis.auto_reframe` | C | Follow a gradient-energy centroid with center bias and temporal smoothing, then crop/scale to a target aspect. |
| `camera_shake` | `nw.video.motion.camera_shake` | C | Animate position, rotation and zoom from seeded smooth control noise. |
| `grow` | `nw.video.motion.grow` | C | Animate scale between endpoints over clip time with selectable easing. |
| `shrink` | `nw.video.motion.shrink` | C | Animate scale downward or between authored endpoints over clip time. |
| `move` | `nw.video.motion.move` | C | Animate translation between authored endpoints. |
| `spin` | `nw.video.motion.spin` | C | Animate rotation around an anchor. |
| `spacer` | `nw.video.layout.spacer` | C | Fit the image inside margins with rounded corners and optional background fill. |
| `wiggle` | `nw.video.motion.wiggle` | C | Apply seeded smooth position/rotation/scale jitter. |
| `auto_align` | `nw.video.analysis.alpha_align` | C | Find visible alpha bounds and align them to a chosen canvas edge or center. |
| `cineon_converter` | `nw.video.color.cineon` | C | Convert a declared Cineon log encoding to/from linear light. |
| `clone` | `nw.video.tile.clone` | C | Repeat a fitted picture in a rows/columns grid with gaps and alternating mirror options. |
| `stroke` | `nw.video.alpha.outline` | C | Outline the visible alpha inside, outside or centered on the boundary. |
| `metadata_burnin` | `nw.video.text.burnin` | P | Burn text or resolved media/sequence metadata into the picture. |
| `echo` | `nw.video.time.echo` | C | Composite finite earlier/later samples of the preceding processed layer in clip time, with decay and a chosen blend operator. |
| `lightning` | `nw.video.generate.lightning` | P | Generate animated branched lightning paths with core and glow. |
| `cell_pattern` | `nw.video.generate.cellular_noise` | C | Generate seeded cellular/Worley-style patterns from bounded neighboring sites. |
| `checkerboard` | `nw.video.generate.checkerboard` | P | Draw antialiased alternating checker cells with color/transparent choices. |
| `ellipse` | `nw.video.generate.ellipse_ring` | P | Draw an elliptical ring with independently styled interior. |
| `paint_bucket` | `nw.video.fill.connected` | C | Fill the connected component around an authored seed under a color/alpha tolerance. |
| `write_on` | `nw.video.paint.write_on` | P | Accumulate an animated brush path up to the current time. |
| `alpha_glow` | `nw.video.light.alpha_glow` | C | Glow around alpha edges with an inner-to-outer color treatment. |
| `block_dissolve` | `nw.video.mask.block_reveal` | C | Reveal/hide a layer in seeded block order. |
| `directional_blur_legacy` | `nw.video.blur.directional` | N | Blur along an authored direction. |
| `gaussian_blur_legacy` | `nw.video.blur.gaussian` | N | Apply Gaussian blur with independent horizontal/vertical amount and declared edge treatment. |
| `gradient_wipe_legacy` | `nw.video.mask.luma_wipe` | C | Remove alpha according to a track/source luma field and completion. |
| `linear_wipe_legacy` | `nw.video.mask.linear_wipe` | C | Remove alpha behind an angled sweeping boundary. |
| `magnify_legacy` | `nw.video.geometry.loupe` | C | Magnify a circular or rectangular region and composite it over the original. |
| `mosaic_legacy` | `nw.video.pixelate` | C | Replace the image with a grid of reduced-resolution color blocks. |
| `noise_legacy` | `nw.video.noise.grain` | C | Add mono or color grain with explicit scale, seed and clipping policy. |
| `twirl_legacy` | `nw.video.geometry.twirl` | C | Rotate sampling coordinates by a radial falloff around a center. |
| `block_motion` | `nw.transition.cards.block_motion` | C | Animate a grid of image cards with staggered ordering. |
| `flip_motion` | `nw.transition.cards.flip` | C | Flip a planar picture card to reveal the other side. |
| `fold_motion` | `nw.transition.cards.fold` | C | Fold two hinged picture halves to reveal the next image. |
| `pop_motion` | `nw.transition.cards.pop` | C | Pop the incoming picture into place with overshoot. |
| `pull_motion` | `nw.transition.cards.pull` | C | Pull an incoming/outgoing picture through scale/depth motion. |
| `spin_motion` | `nw.transition.cards.spin` | C | Spin the image card around an authored center during replacement. |
| `spring_motion` | `nw.transition.motion.spring` | C | Move the incoming image into place with a damped spring trajectory. |
| `travel_motion` | `nw.transition.cards.travel` | C | Move a planar picture through an authored perspective trajectory. |
| `additive_dissolve` | `nw.transition.dissolve.additive` | C | Overlap images through an additive-light dissolve. |
| `blur_dissolve` | `nw.transition.dissolve.blur` | C | Blur both images around the transition midpoint while crossfading. |
| `burn_alpha` | `nw.transition.dissolve.alpha_burn` | C | Burn away outgoing coverage with a shaped luma/color mask. |
| `cross_dissolve` | `nw.transition.dissolve.cross` | C | Crossfade the two images. |
| `dip_to_black` | `nw.transition.dip.black` | C | Fade outgoing to black then black to incoming. |
| `dip_to_color` | `nw.transition.dip.color` | C | Fade through an authored opaque color. |
| `dip_to_white` | `nw.transition.dip.white` | C | Fade outgoing to white then white to incoming. |
| `film_dissolve` | `nw.transition.dissolve.perceptual` | C | Blend in a declared nonlinear/gamma-adjusted domain for a film-like dissolve. |
| `luma_fade` | `nw.transition.dissolve.luma` | C | Reveal incoming through a soft luminance-driven threshold. |
| `morph_cut` | `nw.transition.flow.morph` | C | Interpolate motion between the corresponding outgoing/incoming transition frames. |
| `mosaic_transition` | `nw.transition.look.mosaic` | C | Increase/decrease pixelation around an A/B dissolve. |
| `chaos` | `nw.transition.look.chaos` | C | Add seeded staggered movement/distortion around the transition. |
| `earthquake` | `nw.transition.look.earthquake` | C | Shake, rotate and zoom the picture near the transition midpoint. |
| `flicker` | `nw.transition.look.flicker` | C | Modulate brightness while changing between images. |
| `glass` | `nw.transition.look.glass` | C | Refract the picture through a procedural glass-cell field. |
| `glitch` | `nw.transition.look.glitch` | C | Use displaced horizontal bands and RGB separation during the cut. |
| `grunge` | `nw.transition.look.grunge` | C | Reveal the next picture through a distressed procedural mask. |
| `kaleidoscope` | `nw.transition.look.kaleidoscope` | C | Fold angular sectors into a mirrored kaleidoscopic transition. |
| `liquid_distortion` | `nw.transition.look.liquid` | C | Apply an animated smooth liquid-like coordinate field while crossfading. |
| `tv_power` | `nw.transition.look.tv_power` | C | Contract/expand the picture with a TV-switch light line/flash. |
| `vhs_damage` | `nw.transition.look.vhs_damage` | C | Combine horizontal tearing, chroma offsets, grain and scanline treatment. |
| `vr_chroma_leaks` | `nw.transition.vr.light_leaks` | C | Overlay wrapping colored light blobs during an A/B dissolve. |
| `vr_gradient_wipe` | `nw.transition.vr.gradient_wipe` | C | Reveal the next sphere with a vertical gradient and periodic longitude variation. |
| `vr_iris_wipe` | `nw.transition.vr.iris` | C | Reveal the next image by angular distance from a spherical center. |
| `vr_light_leaks` | `nw.transition.vr.light_leaks` | C | Overlay wrapping colored light blobs during an A/B dissolve. |
| `vr_light_rays` | `nw.transition.vr.light_rays` | C | Add wrapping angular light rays around the midpoint dissolve. |
| `vr_mobius_zoom` | `nw.transition.vr.zoom_twist` | C | Apply a longitude-wrapping radial zoom/twist during transition. |
| `vr_random_blocks` | `nw.transition.vr.blocks` | C | Reveal by seeded horizontal-wrapping block order. |
| `vr_spherical_blur` | `nw.transition.vr.blur_dissolve` | C | Blur an A/B mix with the source feature’s flat approximation, optionally choose the spherical blur mode. |
| `burn_chroma` | `nw.transition.look.chroma_burn` | C | Add chromatic light around the midpoint blend. |
| `chroma_leak` | `nw.transition.look.light_leak` | C | Wash the midpoint blend with moving colored gradient blobs. |
| `cross_zoom` | `nw.transition.blur.zoom` | C | Blur radially by scaling samples about a center while crossfading. |
| `directional_blur_transition` | `nw.transition.blur.directional` | C | Apply directional blur around the midpoint crossfade. |
| `flare` | `nw.transition.light.flare` | C | Add an authored light core, halo and streak during the cut. |
| `flash` | `nw.transition.light.flash` | C | Flash an authored color over the transition. |
| `glow` | `nw.transition.light.glow` | C | Add thresholded blurred light around the transition midpoint. |
| `lens_blur` | `nw.transition.blur.aperture` | C | Apply aperture blur to transition inputs under a midpoint envelope, then crossfade. |
| `light_leak` | `nw.transition.look.light_leak` | C | Wash the midpoint blend with moving colored gradient blobs. |
| `light_sweep` | `nw.transition.light.sweep` | C | Sweep a moving band of light across the images. |
| `phosphor` | `nw.transition.look.phosphor` | C | Create a channel-biased glow/persistence-style cut. |
| `radial_blur` | `nw.transition.blur.angular` | C | Blur angularly around a center during the cut. |
| `ray` | `nw.transition.light.rays` | C | Overlay radial shafts of light during the transition. |
| `solarize` | `nw.transition.look.solarize` | C | Apply a thresholded nonlinear color inversion during the blend. |
| `stripe` | `nw.transition.light.stripes` | C | Move bands of colored light across the A/B blend. |
| `zoom_blur` | `nw.transition.blur.zoom` | C | Blur radially by scaling samples about a center while crossfading. |
| `roll_3d` | `nw.transition.cards.roll_3d` | C | Roll a planar picture card in perspective with front/back visibility. |
| `film_roll` | `nw.transition.motion.film_roll` | C | Move/rotate pictures like frames on a film strip. |
| `push` | `nw.transition.motion.push` | C | Push outgoing and incoming images together across the frame. |
| `roll` | `nw.transition.motion.roll` | C | Combine translation and rotation as pictures change. |
| `slide` | `nw.transition.motion.slide` | C | Slide incoming over stationary outgoing image. |
| `split` | `nw.transition.motion.split` | C | Move or reveal outgoing halves away from a chosen split. |
| `stretch` | `nw.transition.motion.stretch` | C | Stretch and translate the pictures during replacement. |
| `whip` | `nw.transition.motion.whip` | C | Perform a fast directional move with directional/motion blur. |
| `motion_camera` | `nw.transition.motion.camera_style` | C | Apply authored camera-like transforms to the two images. |
| `motion_tween` | `nw.transition.motion.tween` | C | Interpolate parameter-defined transforms between the two images. |
| `shape_dissolve` | `nw.transition.mask.shape_cells` | C | Reveal through a grid of growing circle/box/diamond/star shapes. |
| `shape_flow` | `nw.transition.mask.shape_flow` | C | Move and enlarge a shape from an edge to reveal the next image. |
| `text_animator` | `nw.transition.cells.staggered_slide` | C | Slide rectangular image cells vertically with row/column staggering. |
| `typewriter` | `nw.transition.cells.reading_reveal` | C | Reveal rectangular image cells in reading order, optionally with a caret block. |
| `spin_3d` | `nw.transition.cards.spin_3d` | C | Spin a textured planar card to reveal the next image. |
| `spinback_3d` | `nw.transition.cards.spinback` | C | Spin the card away/back in depth while switching images. |
| `frame` | `nw.transition.cards.frame_parts` | C | Separate frame-shaped picture elements to reveal the incoming image. |
| `louver` | `nw.transition.cards.louver` | C | Rotate a set of hinged image slats. |
| `mirror_transition` | `nw.transition.cards.mirror` | C | Animate mirrored image-card arrangements during the transition. |
| `page_peel` | `nw.transition.page.peel` | C | Peel a page corner over a curved roll, exposing a shaded back and incoming image. |
| `slice` | `nw.transition.cards.slices` | C | Move picture slices with staggered timing and optional depth. |
| `wave` | `nw.transition.cards.wave` | C | Animate picture sections along a wave of displacement/rotation. |
| `clock_wipe` | `nw.transition.wipe.clock` | C | Sweep an angular reveal around an authored center. |
| `linear_wipe` | `nw.transition.wipe.linear` | C | Sweep an angled boundary across the picture with adjustable feather. |
| `neon_wipe` | `nw.transition.wipe.neon` | C | Reveal along a moving edge with colored core and glow. |
| `panel_wipe` | `nw.transition.wipe.panels` | C | Reveal through direction-aligned strips with staggered delay. |
| `plateau_wipe` | `nw.transition.wipe.plateau` | C | Reveal with a stepped boundary and symmetric strip delays. |
| `radial_wipe` | `nw.transition.wipe.corner_angle` | C | Sweep an angular reveal originating at a selected frame corner. |
| `soft_wipe` | `nw.transition.wipe.linear` | C | Sweep an angled boundary across the picture with adjustable feather. |
| `star_wipe` | `nw.transition.wipe.star` | C | Reveal through a growing rotated star shape. |
| `stretch_wipe` | `nw.transition.wipe.stretch_edge` | C | Reveal while stretching incoming content near the moving boundary. |
| `additive_dissolve_legacy` | `nw.transition.dissolve.additive` | C | Overlap images through an additive-light dissolve. |
| `barn_doors` | `nw.transition.wipe.barn_doors` | C | Reveal outward/inward from a central horizontal or vertical line. |
| `center_split` | `nw.transition.motion.split` | C | Move or reveal outgoing halves away from a chosen split. |
| `clock_wipe_legacy` | `nw.transition.wipe.clock` | C | Sweep an angular reveal around an authored center. |
| `cross_dissolve_legacy` | `nw.transition.dissolve.cross` | C | Crossfade the two images. |
| `cross_zoom_legacy` | `nw.transition.motion.zoom_cross` | C | Scale images around center while crossfading. |
| `dip_to_black_legacy` | `nw.transition.dip.black` | C | Fade outgoing to black then black to incoming. |
| `dip_to_white_legacy` | `nw.transition.dip.white` | C | Fade outgoing to white then white to incoming. |
| `film_dissolve_legacy` | `nw.transition.dissolve.perceptual` | C | Blend in a declared nonlinear/gamma-adjusted domain for a film-like dissolve. |
| `inset` | `nw.transition.wipe.corner_box` | C | Reveal with an expanding aspect-correct rectangular metric from a corner. |
| `iris_box` | `nw.transition.wipe.iris_box` | C | Reveal with a growing rectangle around a center. |
| `iris_cross` | `nw.transition.wipe.iris_cross` | C | Reveal with expanding crossed bars. |
| `iris_diamond` | `nw.transition.wipe.iris_diamond` | C | Reveal with an expanding diamond. |
| `iris_round` | `nw.transition.wipe.iris_round` | C | Reveal with an expanding circle/ellipse. |
| `non_additive_dissolve` | `nw.transition.dissolve.non_additive` | C | Use a luminance-dependent nonlinear dissolve. |
| `push_legacy` | `nw.transition.motion.push` | C | Push outgoing and incoming images together across the frame. |
| `radial_wipe_legacy` | `nw.transition.wipe.clock` | C | Sweep an angular reveal around an authored center. |
| `slide_legacy` | `nw.transition.motion.slide` | C | Slide incoming over stationary outgoing image. |
| `split_legacy` | `nw.transition.motion.split` | C | Move or reveal outgoing halves away from a chosen split. |
| `whip_legacy` | `nw.transition.motion.whip` | C | Perform a fast directional move with directional/motion blur. |
| `wipe` | `nw.transition.wipe.linear` | C | Sweep an angled boundary across the picture with adjustable feather. |
| `band_slide` | `nw.transition.motion.band_slide` | C | Slide horizontal picture bands with staggered timing. |
| `checker_wipe` | `nw.transition.wipe.checker` | C | Reveal alternating checker cells. |
| `cube_spin` | `nw.transition.motion.cube_style` | C | Create the source feature’s cube-like scale/swap transition. |
| `flip_over` | `nw.transition.motion.flip_style` | C | Collapse and expand image scale around a flip point. |
| `gradient_wipe` | `nw.transition.wipe.gradient` | C | Reveal through a procedural or supplied luminance-gradient field. |
| `page_turn` | `nw.transition.page.turn` | C | Turn a page using a diagonal mask/warp with shading. |
| `venetian_blinds` | `nw.transition.wipe.blinds` | C | Reveal in repeated horizontal or vertical strips. |
| `graphic_text` | `nw.graphic.text_layer` | P | Render structured rich text layers with per-run styles, paragraph/vertical layout, fills, two strokes, background, shadow and transforms. |
| `graphic_shape` | `nw.graphic.shape_layer` | P | Render rectangle, ellipse, polygon and authored path layers with fills/gradients, multiple strokes, background, shadow and transforms. |
