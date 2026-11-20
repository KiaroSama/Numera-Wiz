> Generated view of [Numera-Wiz-Implementation-Mandate-EN.md](../Numera-Wiz-Implementation-Mandate-EN.md), revision 7.0. Main SHA-256: `126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06`. Normative text is unchanged; fragment links point to the complete directive.

## Annex B. Donor capability decisions

This annex assigns every source effect/transition/intrinsic/graphic entry a functional design disposition.
The authoritative combined machine-readable ledger is `research_v7/donor-effects-coverage-v7.csv`.
It retains all 303 FilmCraft and 307 EffectCraft source records, their individual route,
parameter differences, resources, source/manual evidence and implementation/qualification obligations.
It has no unassigned rows. These are proposed source-informed routes; each implementation must verify
its actual inputs, graph semantics and admitted output domain. Source coverage is not evidence of
runtime correctness or a claim that the application is already built.

The combined design has 481 canonical target identities, including unavailable semantic functions
and typed data records. Ninety EffectCraft proposals resolve to shared descriptors, explicit variants
or a selector split; 29 more keep distinct context while sharing a primitive implementation.
Do not interpret 610 source rows or 481 identities as a count of delivered effects.

| Disposition | FilmCraft records | EffectCraft records | Combined records |
|---|---:|---:|---:|
| `native` | 52 | 45 | 97 |
| `composed` | 235 | 211 | 446 |
| `component` | 15 | 21 | 36 |
| `application` | 0 | 11 | 11 |
| `unsupported` | 1 | 19 | 20 |

The 20 unsupported rows are precise primary semantic functions: one FilmCraft DeReverb and
19 EffectCraft advanced analysis/simulation functions. Supported parent workflows and named
alternatives remain usable. The 41 partial EffectCraft records are individually described below.
Different donor numerical output alone is not grounds for excluding an ordinary supported task.

### Canonical identity and variant rules

#### Apply these rules when generating the combined registry

1. Normalize the proposal namespace to `nw.*`. Do not create two capabilities differing only by that prefix.
2. Resolve every EffectCraft proposal to `unified_target_capability_id`. The row's `unified_contract` and `required_variant_or_parameter_migration` define the combined design. Do not overwrite one donor's controls with the other donor's column values.
3. Apply **only** the explicit `filmcraft_ids_to_remap` field to FilmCraft IDs. The five ordinary iris/shape-wipe variants become one `nw.transition.wipe.iris` descriptor with a validated shape enum. Preserve their names as presets/UI aliases with the correct parameters.
4. `filmcraft_related_capability_ids` is a comparison/reference field. It is **not** an instruction to alias every listed ID. Distinct-context rows deliberately preserve graph placement, input preconditions or meaningful function.
5. The selective-retention row uses a concrete default RGB target and a conditional hue target. RGB distance and hue distance have separate schemas. `additional_conditional_target_ids` must be retained; do not collapse these into one unlabeled tolerance control.
6. Shared descriptors implement the union through typed variants, not inert controls. A field that applies only to one kernel, color model, contour or processor is rejected or hidden in other variants. Native defaults and units remain explicit.
7. Implement each `shared_lowering_owner` once. Distinct effect descriptors, presets, intrinsic properties and transitions may compose the same primitive. A distinct public capability is not a reason to create a second pixel/DSP engine or copy a filter compiler.

The registry descriptor/version owns GUI controls, browser controls, CLI/MCP schema, defaults, plan generation and qualification. Source IDs/defaults remain provenance unless a separate compatibility feature is authorized.

#### Reconciliation result

| Resolution | EffectCraft proposal IDs | Meaning |
|---|---:|---|
| Shared descriptor | 35 | Individually compared matching tasks and control domains. Matching spelling alone was not the rule. |
| Shared descriptor with explicit variants | 54 | Differently named equivalents or useful richer contracts; migration/variant fields are stated per row. |
| Split by explicit selector | 1 | RGB retention and hue retention remain separately typed capabilities. |
| Distinct context, shared primitive | 29 | The task/placement differs, while a concrete lower-level implementation is shared. |
| Distinct capability | 147 | No equivalent FilmCraft task is claimed; common primitive/compiler infrastructure is still reused. |
| Typed project record | 11 | Parameter/result containers, not fake media effects or automatic analysis. |
| Precise unsupported semantic identity | 19 | Preserve the unavailable primary feature and separately named useful alternatives. |

After the five iris aliases are reconciled, the combined design has **481 distinct target IDs** across both donor ledgers, including data records and unsupported identities. This is an inventory sanity check, not a count of implemented effects. `cross-donor-capability-aliases-summary.json` records the derivation; every row is still `NOT_IMPLEMENTED_OR_EXECUTED`.

#### Examples that must not lose semantics

The v7 temporal correction replaces the unqualified source-time Echo alias with explicit clip-clock sampling of the preceding processed layer. Posterize Time uses the same upstream-input contract and retains its existing `nw.video.time.posterize` to `nw.video.time.hold_rate` resolution. Parameter clocks and requested sample clocks are distinct. Numera deliberately includes earlier temporal nodes in an acyclic upstream prefix; FilmCraft's skipped-prior-temporal limitation is source provenance, not an inherited canonical restriction. No alias ID, resolution, count or runtime status changes. [FilmCraft frame access](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/mod.rs#L38-L121) · [EffectCraft temporal input](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/time_fx.rs#L159-L220)

| Compared entries | Concrete decision |
|---|---|
| Echo | One processed-layer echo with signed clip-clock taps, explicit average normalization, finite endpoints and admitted sample schedules; source-clock behavior requires a named variant. |
| Posterize Time / hold rate | One upstream-layer hold capability at quantized clip-relative times; preserve output cadence and duration, with explicit phase/rounding/endpoints and admitted animated schedules. |
| Gaussian, channel and directional blur | Share native blur implementations. Directional blur has explicit `dblur` versus rotated-axis-Gaussian kernel variants; ordinary Gaussian remains one `gblur`. |
| Mosaic | One block-pixelation capability, using downsample/upscale. Picture repetition belongs to a separate tiling capability. |
| Black & White | One monochrome descriptor with `luminance_matrix` and `hue_mix` variants. The simple matrix mode does not incur the selective mode's extra stages. |
| Noise and Add Grain | Shared noise capability with `sample_noise` and `grain` patterns. Clipping, headroom preservation and modulo-one wrapping are separate overflow variants: the donors' `clip=false` branches differ. A native fast route is admitted only for matching semantics. HSL noise remains a distinct domain-specific processor. |
| RGB/hue selective retention | Default RGB and explicit hue target; native YUV-derived `hsvhold` is not advertised as the same metric as HSL angular distance. |
| Two donors' Extract | FilmCraft's monochrome range visualization preserves alpha; EffectCraft's range key changes coverage. These stay distinct. |
| Two donors' Plane to Sphere | FilmCraft inserts a flat plane into a sphere; the reviewed EffectCraft entry is a tiny-planet projection. These stay distinct despite the matching label. |
| Inserted transform/timewarp versus intrinsic properties | Reuse transform/time-map primitives while retaining clip stack position, source/effect clock and intrinsic state. |
| Path stroke versus alpha outline | Authored path painting and deriving an outline from existing alpha remain distinct. |
| Iris/circle/box/cross/diamond/star reveal | One validated shape-wipe descriptor with explicit shape-specific fields. Corner-origin inset and corner-angle wipe remain separate. |
| Point/face controls | Typed project data remains independent of rendering and unavailable automatic recognition. |

These are selected implementation contracts, not legacy preset conversion guarantees. Qualification still checks each active variant's options, precision, alpha, timing, resource use and parity.

### FilmCraft decisions

The tables below use the reconciled canonical identities. Per-donor reports retain their original review proposals as provenance; where an ID differs, the combined ledger and explicit alias/variant rules are authoritative. They do not discard the per-row semantic/resource restrictions.

**Source revision:** `d643a886650bfbf16f24762274054575aa1e8f20`. **FFmpeg revision:** `946fcce07b6dcd0331c8cc609192aeff5e1924f8`. **Date:** 2026-10-21.

This is a source-informed **design disposition for every one of the 303 FilmCraft effect/transition/intrinsic/graphic-layer inventory rows**. The complete machine-readable contracts are in `filmcraft-functional-mapping-final.csv`. It does not assert implementation or runtime qualification. Every row has `runtime_status=NOT_IMPLEMENTED_OR_EXECUTED`.

There are 52 native, 235 composed, 15 component-qualified and 1 precisely unsupported proposed design dispositions. No inventory row is unassigned or left as an `investigate` placeholder. Source semantics, parameter schema, selected lowering and resource assumptions must still be checked together during implementation; an assigned family or present filter name does not establish a complete route. Every implementation must pass its stated build/correctness/performance gates, and the optical-flow composition has a mandatory focused CI qualification task.

#### Binding interpretation

The CSV's `target_capability_id` names a Numera effect, intrinsic property, graphic layer or transition capability, **not an action command**. Repeated target IDs intentionally merge source aliases that provide the same functional behavior. Distinct functions such as RGB color isolation versus hue isolation, crop-as-mask versus output-size crop, intrinsic gain versus inserted gain, and clock wipe versus corner-angle wipe remain distinct.

The source descriptors and defaults are provenance. New Numera controls use explicit native units and useful native semantics. Source-private rendering algorithms, RNG sequences, default values and native project/preset compatibility are not automatically part of the application API. This does not authorize silent effect removal. Every supported design still needs a working UI/control flow and equal CLI/MCP access; failures to implement a composition cannot be relabeled mathematical incompatibility without review.

`native` means a dedicated native route is selected; normal format/alpha/time adaptation may surround it. `composed` means a concrete bounded graph/parameter-generated vector or control artifact is selected. `component` identifies a real build/asset prerequisite such as Cairo, libvidstab, librubberband or HRTF data. `unsupported` identifies only the specified semantic function for which this review established no admitted practical route. It does not exclude a wider family.

All builds, ordinary media tests and performance qualification belong in GitHub CI. GPU qualification uses the owner's matching CI-built artifact. This review ran no application code, media jobs or builds.

#### Findings that materially change the implementation

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

#### Shared implementation boundaries that avoid another impractical rewrite

##### Native precision is checked per route

The pinned `perspective` implementation only advertises 8-bit picture formats. Its existence does not authorize silently quantizing the whole render. A bounded inverse-homography `geq` sampler is the specified float alternative; it performs a constant number of source lookups per output pixel. `remap` uses integer coordinate maps and cannot be treated as fractional precision by assertion. Alpha outside the plane needs explicit masks/guards because `geq` normally clamps out-of-range sampling. [Perspective source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_perspective.c) · [Geq source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_geq.c)

The pinned `convolve` route supports integer picture formats through 16-bit and normalizes its impulse by the coefficient sum. Aperture blur therefore has a concrete native route, with kernel alignment, padding and HDR range conversion exposed. The ordinary Gaussian family still uses `gblur`; it must not pay aperture-convolution costs. [Convolution source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_convolve.c) · [Gaussian source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_gblur.c)

The `varblur` route accepts a synchronized guide and supports GBR(P/A)F32. The guide must use the negotiated picture layout, with the scalar control replicated into each processed plane; the pinned common-format list has no GRAYF32. Its `min_r`/`max_r` labels are native controls, not a proven physical zero-to-R radius. Source `radiusf` derives from doubled native bounds and guide values, and `min_r=0` still has nonzero support. Specify source-derived guide calibration and exact original-image bypass for zero blur/focus, then qualify impulse response, edges and tiny dimensions in CI. Its summed-area tables alone allocate `nb_planes*(W+1)*(H+1)*(4 bytes if depth<=8 else 8)`, even for unprocessed planes. A single native processor can replace a large Gaussian stack while still requiring this material memory budget. [Variable blur formats and processing](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_varblur.c#L75-L245) · [Buffer allocation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_varblur.c#L344-L359)

##### Procedural effects remain bounded

Native `perlin`/`noise`, small masks, LUTs, vector gradients and native blur cover ordinary procedural looks. A custom `geq` formula is justified for a small fixed coordinate field, selection mask or local gather. It is not a license to replicate all donor internal math. Distinguish O(1) coordinate sampling, fixed small gathers, and high-cost growing searches in the planner. Native morphology handles broad alpha outlines; large repeated-overlay or exact distance-transform constructions are not defaults. [Morphology source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_morpho.c)

Procedural seeds define repeatable Numera output. Optional blur samples, ray taps, slats or cells have explicit quality/resource parameters. A source-defined large setting is admitted according to the actual plan budget; it is not silently clamped or advertised as real-time. Interactive draft resolution is labeled, while final export follows the reviewed quality definition.

##### Typography remains complete without a second pixel engine

The source graphic layer includes rich runs, alignment/justification, tracking, kerning/ligatures, vertical text, baseline/caps, boxes, gradient fill, multiple strokes, background, shadow and transforms. `drawtext` alone does not prove those controls work. Use native text/libass where it fits; for advanced typography, Rust can shape/layout fonts into **vector glyph paths** and FFmpeg `drawvg` rasterizes them. This is parameter/geometry preparation, not a Rust rasterizer. Cache glyph/layout/path artifacts; declare fonts and the standard Cairo-enabled component. The FFmpeg vector language is a path renderer, not an assumed rich-text shaping service. Its actual formats are RGB32, 0RGB32, RGB565 and X2RGB10; Cairo ARGB32 expects premultiplied alpha. Rasterize bounded coverage/graphic layers separately and compose them in the chosen working domain. A mask-first path can apply authored HDR colors after rasterization; declare coverage precision and alpha conversion. Do not send the whole float/HDR base picture through Cairo to add a title. [Graphic source](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/project/src/graphic.rs) · [Drawvg formats](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_drawvg.c#L2675-L2694)

##### Audio delay and limiting use honest native definitions

Echoes have explicit finite tail/noise-floor semantics. A native circular delay, finite-tap `aecho` or partitioned `afir` plan replaces dense high-order `aiir`; colored/saturated echoes remain available without pretending the finite graph is an infinite nonlinear feedback loop. Do not force users to lose ordinary echo/reverb functions because the source algorithm differs. The concrete bound includes `afir`'s maximum supported IR duration. [Echo source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_aecho.c) · [FIR source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_afir.c)

Limiter output gain and latency are explicit. `alimiter` automatic normalization must not silently defeat a ceiling. A true-peak export mode is a measurable contract using declared oversampling/limiting/resampling and a final FFmpeg true-peak measurement. If it misses the ceiling, the declared policy rerenders with a margin or fails; an unchecked result is never called verified true-peak. [Limiter source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/af_alimiter.c)

#### Motion morph: concrete design requiring a focused CI qualification

The FilmCraft source applies optical interpolation to its two transition images. FFmpeg `minterpolate` derives interpolation alpha from neighboring input presentation timestamps, rather than requiring a fixed 50% blend. This allows a one-process paired-stream design; it is a source-derived proposal, not a tested command.

For output frame time `t_n = n/F`, desired progress `p_n` in `[0,1]`, and `0 < delta < 1/(2F)`, assign the paired input frames:

```text
A_n.pts = t_n - p_n * delta
B_n.pts = t_n + (1 - p_n) * delta
```

Both per-input sequences stay monotonic for any bounded progress sequence, and each pair surrounds the desired output time. The timestamps encode `p_n` as the interpolation fraction. Rebase/add lead-in to keep the desired input domain; use a sufficiently precise time base, interleave the two streams by timestamp, and request output rate F from `minterpolate` with bilateral motion estimation and deliberate scene-cut policy. This produces O(N) input frames, avoiding N squared intermediate-frame generation or one process per output frame. The exact endpoints can bypass the morph stage.

The qualification task must prove frame selection, **multi-frame context and neighboring-pair estimation behavior**, internal alpha quantization, edge-frame padding/flush, EOF duration and scene-cut behavior using moving fixtures. The pinned route's format limitations must be explicit: ordinary resolved opaque 8-bit pictures are the initial native domain; high-depth or transparent-layer motion interpolation needs a separately qualified route and cannot be silently flattened/quantized. The design remains `NOT_IMPLEMENTED_OR_EXECUTED` until this gate passes. If the focused spike fails, withhold this specialized motion-morph variant, record the failed boundary and retain the ordinary supported transitions; do not start a new first-party optical-flow renderer. This spike is not a prerequisite for the first basic-editor milestone and is not a validated default. [Source interpolation and frame scheduling](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_minterpolate.c#L1074-L1208) · [FilmCraft transition dispatch](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/transitions/dissolve.rs)
#### Complete FilmCraft source index

| Source ID | Source label | Canonical capability | Design | Defined task |
|---|---|---|---|---|
| `brightness_contrast` | Brightness & Contrast | `nw.video.color.brightness_contrast` | composed | Adjust picture brightness and contrast in an explicitly selected signal domain. |
| `tint` | Tint | `nw.video.color.two_color_tint` | composed | Map luminance between chosen dark/light colors and mix with the original. |
| `color_balance` | Color Balance | `nw.video.color.tonal_balance` | composed | Apply separate RGB shadow, midtone and highlight corrections with optional luminosity preservation. |
| `leave_color` | Leave Color | `nw.video.color.hue_isolation` | composed | Keep a selected hue neighborhood colored and desaturate other hues. |
| `change_to_color` | Change to Color | `nw.video.color.hue_replacement` | composed | Shift hues selected by a circular hue key toward a target hue while retaining chosen luminance/saturation. |
| `lumetri` | Lumetri Color | `nw.video.color.grading_stack` | composed | A sectioned color-grading stack with primary corrections, creative looks, curves, wheels, HSL secondary selection, LUTs, sharpening and vignette. |
| `gaussian_blur` | Gaussian Blur | `nw.video.blur.gaussian` | native | Apply Gaussian blur with independent horizontal/vertical amount and declared edge treatment. |
| `directional_blur` | Directional Blur | `nw.video.blur.directional` | native | Blur along an authored direction. |
| `sharpen` | Sharpen | `nw.video.sharpen.standard` | native | Sharpen using a native unsharp residual. |
| `unsharp_mask` | Unsharp Mask | `nw.video.sharpen.thresholded` | composed | Sharpen only residuals exceeding an authored threshold. |
| `camera_blur` | Camera Blur | `nw.video.blur.defocus_simple` | native | Provide a simple image defocus blur. |
| `black_white` | Black & White | `nw.video.color.monochrome` | composed | Convert picture color to a declared luminance grayscale while retaining alpha. |
| `color_pass` | Color Pass | `nw.video.color.rgb_isolation` | composed | Keep colors near an RGB key and desaturate the remainder, optionally reversing the selection. |
| `gamma_correction` | Gamma Correction | `nw.video.color.gamma` | composed | Adjust signal gamma with explicit exponent convention. |
| `invert` | Invert | `nw.video.color.invert_channels` | composed | Invert selected RGB channels or alpha and blend with the original. |
| `levels` | Levels | `nw.video.color.levels` | composed | Remap input black/white and midpoint gamma into output black/white. |
| `proc_amp` | ProcAmp | `nw.video.color.proc_amp` | composed | Adjust brightness, contrast, hue and saturation as one convenient processor. |
| `extract` | Extract | `nw.video.color.luma_range_visualizer` | composed | Replace RGB with a monochrome luma-range selection while preserving alpha. |
| `posterize` | Posterize | `nw.video.color.posterize` | composed | Quantize each color component to a chosen number of levels. |
| `mosaic` | Mosaic | `nw.video.pixelate` | composed | Replace the image with a grid of reduced-resolution color blocks. |
| `find_edges` | Find Edges | `nw.video.edges` | composed | Display an edge-strength image, optionally invert and mix with the source. |
| `emboss` | Emboss | `nw.video.relief.emboss` | composed | Create monochrome directional relief and blend with source. |
| `replicate` | Replicate | `nw.video.tile.replicate` | composed | Repeat the complete picture in a regular grid. |
| `strobe` | Strobe Light | `nw.video.light.strobe` | composed | Gate a color wash periodically in clip time. |
| `noise` | Noise | `nw.video.noise.grain` | composed | Add mono or color grain with explicit scale, seed and clipping policy. |
| `median` | Median | `nw.video.denoise.median` | native | Remove local impulsive image noise with a median neighborhood. |
| `crop` | Crop | `nw.video.mask.crop` | composed | Cut away canvas edges with optional feather; optionally enlarge the retained rectangle back to the canvas. |
| `horizontal_flip` | Horizontal Flip | `nw.video.geometry.flip_horizontal` | composed | Reflect the picture horizontally. |
| `vertical_flip` | Vertical Flip | `nw.video.geometry.flip_vertical` | composed | Reflect the picture vertically. |
| `edge_feather` | Edge Feather | `nw.video.mask.edge_feather` | composed | Feather opacity inward from the frame rectangle. |
| `transform` | Transform | `nw.video.geometry.transform` | composed | Transform a layer using anchor, position, independent scale, rotation, skew and opacity. |
| `mirror` | Mirror | `nw.video.geometry.mirror_line` | composed | Reflect one side of an authored line to the other side. |
| `offset` | Offset | `nw.video.geometry.wrap_offset` | composed | Wrap-shift the picture horizontally/vertically and mix with original. |
| `lens_distortion` | Lens Distortion | `nw.video.geometry.radial_lens` | composed | Apply radial lens distortion with an adjustable optical center and optional original mix. |
| `twirl` | Twirl | `nw.video.geometry.twirl` | composed | Rotate sampling coordinates by a radial falloff around a center. |
| `wave_warp` | Wave Warp | `nw.video.geometry.wave` | composed | Displace the image with an animated directional sine wave. |
| `basic_3d` | Basic 3D | `nw.video.geometry.planar_3d` | composed | Tilt/swivel one planar layer with perspective and depth placement. |
| `drop_shadow` | Drop Shadow | `nw.video.shadow.drop` | composed | Cast a colored blurred alpha shadow at an angle/distance, optionally show shadow only. |
| `bevel_alpha` | Bevel Alpha | `nw.video.relief.alpha_bevel` | composed | Shade the alpha boundary as a lit bevel. |
| `ultra_key` | Ultra Key | `nw.video.key.advanced_chroma` | composed | Key a background with matte generation, cleanup, spill reduction and output-preview modes. |
| `track_matte` | Track Matte Key | `nw.video.mask.track_matte` | composed | Use another layer/track alpha or luminance as this layer coverage. |
| `color_key` | Color Key | `nw.video.key.rgb` | composed | Make colors near an RGB key transparent with feather and matte expansion/contraction. |
| `luma_key` | Luma Key | `nw.video.key.luma` | composed | Derive transparency from a luminance threshold/range. |
| `four_color_gradient` | 4-Color Gradient | `nw.video.generate.four_color_field` | composed | Blend four authored colors around defined positions into a smooth background field. |
| `ramp` | Ramp | `nw.video.generate.two_color_ramp` | composed | Generate a linear or radial two-color ramp between points and mix with the picture. |
| `circle` | Circle | `nw.video.generate.circle` | component | Draw a filled antialiased circle. |
| `grid` | Grid | `nw.video.generate.grid` | component | Draw a centered regular grid. |
| `lens_flare` | Lens Flare | `nw.video.light.lens_flare` | composed | Add a procedural light core, halo, streak and ghost elements along a lens axis. |
| `timecode` | Timecode | `nw.video.text.timecode` | component | Burn rational-rate source or sequence timecode into the picture. |
| `clip_name` | Clip Name | `nw.video.text.clip_name` | component | Burn resolved clip name into the picture. |
| `simple_text` | Simple Text | `nw.video.text.burnin` | component | Burn text or resolved media/sequence metadata into the picture. |
| `motion` | Motion | `nw.clip.motion` | composed | Intrinsic placement of source media in the sequence before effect/compositing stages. |
| `opacity` | Opacity | `nw.clip.composite` | composed | Set intrinsic clip opacity and blend mode against lower tracks. |
| `time_remap` | Time Remapping | `nw.clip.time_map` | composed | Map sequence time to clip source time with speed, reverse, holds and exact boundaries. |
| `volume` | Volume | `nw.clip.audio_gain` | native | Apply intrinsic clip gain before its declared downstream mix stage. |
| `channel_volume` | Channel Volume | `nw.clip.channel_gain` | native | Apply intrinsic channel-specific gains without dropping unaddressed channels. |
| `panner` | Panner | `nw.clip.audio_pan` | native | Pan/balance clip audio according to declared source layout and pan law. |
| `constant_power` | Constant Power | `nw.audio.transition.equal_power` | native | Crossfade overlapping audio with an equal-power curve. |
| `constant_gain` | Constant Gain | `nw.audio.transition.linear` | native | Crossfade overlapping audio with complementary linear gains. |
| `exponential_fade` | Exponential Fade | `nw.audio.transition.shaped` | native | Crossfade with a selectable nonlinear easing curve. |
| `amplify` | Amplify | `nw.audio.gain` | native | Add a gain stage at the authored effect-stack position. |
| `channel_mixer_a` | Channel Mixer | `nw.audio.channel_matrix` | native | Mix stereo inputs through an editable matrix with output polarity inversion. |
| `channel_volume_a` | Channel Volume | `nw.audio.channel_gain` | native | Apply per-channel gain at the authored effect-stack position. |
| `deesser` | DeEsser | `nw.audio.cleanup.deess` | native | Reduce excessive sibilance with native de-essing controls. |
| `dynamics_rack` | Dynamics | `nw.audio.dynamics.rack` | composed | Provide independently enabled gate, expander, compressor, limiter, soft clip and output stages. |
| `dynamics` | Dynamics Processing | `nw.audio.dynamics.compressor` | composed | Compress level above threshold with optional soft knee and linked detection. |
| `hard_limiter` | Hard Limiter | `nw.audio.dynamics.limiter` | composed | Limit output peaks with lookahead, and offer an export-measured true-peak ceiling mode. |
| `multiband_compressor` | Multiband Compressor | `nw.audio.dynamics.multiband` | composed | Split into four phase-consistent bands, compress independently or linked, then recombine. |
| `single_band_compressor` | Single-band Compressor | `nw.audio.dynamics.compressor_hard_knee` | composed | Apply hard-knee compression with explicit makeup gain. |
| `tube_compressor` | Tube-modeled Compressor | `nw.audio.dynamics.tube_style` | composed | Combine RMS/soft-knee compression and gentle saturation for a tube-style character. |
| `analog_delay` | Analog Delay | `nw.audio.delay.colored_echo` | composed | Create colored, saturating repeated echoes with an explicit finite tail. |
| `delay` | Delay | `nw.audio.delay.echo` | composed | Create repeated delayed echoes with editable decay and wet/dry balance. |
| `multitap_delay` | Multitap Delay | `nw.audio.delay.multitap` | composed | Mix four independently timed, panned echo taps with individual decay and gain. |
| `bandpass` | Bandpass | `nw.audio.eq.bandpass` | native | Pass an authored frequency band. |
| `bass` | Bass | `nw.audio.eq.low_shelf` | native | Adjust low-frequency shelf gain. |
| `fft_filter` | FFT Filter | `nw.audio.eq.response_curve` | native | Apply an arbitrary bounded frequency/gain curve using native FIR/spectral filtering. |
| `graphic_eq` | Graphic Equalizer (10 Bands) | `nw.audio.eq.graphic_10` | native | Provide a ten-band graphic equalizer. |
| `graphic_eq_20` | Graphic Equalizer (20 Bands) | `nw.audio.eq.graphic_20` | native | Provide a twenty-band graphic equalizer. |
| `graphic_eq_30` | Graphic Equalizer (30 Bands) | `nw.audio.eq.graphic_30` | native | Provide a thirty-band graphic equalizer. |
| `highpass` | Highpass | `nw.audio.eq.highpass` | native | Remove frequencies below cutoff. |
| `lowpass` | Lowpass | `nw.audio.eq.lowpass` | native | Remove frequencies above cutoff. |
| `notch` | Notch Filter | `nw.audio.eq.notch_bank` | native | Apply a finite bank of adjustable frequency notches. |
| `parametric_eq` | Parametric Equalizer | `nw.audio.eq.parametric_full` | native | Provide high/low cuts, low/high shelves, five parametric bands and master gain. |
| `scientific_filter` | Scientific Filter | `nw.audio.eq.classical_iir` | native | Provide Bessel, Butterworth, Chebyshev and elliptic filter designs as stable low-order biquad sections. |
| `simple_notch` | Simple Notch Filter | `nw.audio.eq.notch` | native | Reject one narrow frequency band. |
| `simple_eq` | Simple Parametric EQ | `nw.audio.eq.parametric_band` | native | Boost or cut one parametric band. |
| `treble` | Treble | `nw.audio.eq.high_shelf` | native | Adjust high-frequency shelf gain. |
| `chorus_flanger` | Chorus/Flanger | `nw.audio.modulation.chorus_flanger` | native | Select native chorus or flanger processing. |
| `flanger` | Flanger | `nw.audio.modulation.flanger` | native | Apply a swept short delay with feedback and stereo phase. |
| `phaser` | Phaser | `nw.audio.modulation.phaser` | native | Apply a native moving phase-comb effect. |
| `declicker` | Automatic Click Remover | `nw.audio.cleanup.declick` | native | Remove isolated impulsive clicks using native autoregressive repair. |
| `dehummer` | DeHummer | `nw.audio.cleanup.dehum` | native | Remove mains hum and selected harmonics with adjustable notches. |
| `denoise` | DeNoise | `nw.audio.cleanup.denoise` | native | Reduce broadband/spectral noise using a selected native processor. |
| `dereverb` | DeReverb | `nw.audio.cleanup.dereverb` | unsupported | Late-reverberation suppression using a delayed spectral reverberation model. |
| `speech_enhance` | Enhance Speech | `nw.audio.preset.speech_enhance` | composed | Enhance speech through high-pass, de-mud, presence/air EQ and expander/compressor stages. |
| `convolution_reverb` | Convolution Reverb | `nw.audio.reverb.convolution` | composed | Apply room/cabinet impulse-response convolution with native tone, predelay and wet/dry controls. |
| `studio_reverb` | Studio Reverb | `nw.audio.reverb.studio` | composed | Provide a stereo room/reverb workflow with room-size/tail, damping and wet/dry presets. |
| `surround_reverb` | Surround Reverb | `nw.audio.reverb.surround` | composed | Provide multichannel early/late room response with explicit speaker routing. |
| `binauralizer` | Binauralizer - Ambisonics | `nw.audio.spatial.binaural` | component | Render virtual loudspeaker positions to headphones using explicit HRTF/HRIR assets. |
| `distortion` | Distortion | `nw.audio.distortion.waveshaper` | composed | Apply selectable bounded soft/hard/asymmetric/sine shaping, tone filtering and dry/wet mix. |
| `fill_left` | Fill Left with Right | `nw.audio.channels.right_to_both` | native | Copy right to left while retaining right. |
| `fill_right` | Fill Right with Left | `nw.audio.channels.left_to_both` | native | Copy left to right while retaining left. |
| `guitar_suite` | GuitarSuite | `nw.audio.preset.guitar_suite` | composed | Provide compressor, distortion, amp/cabinet voicing, final tone filter, mix and output sections. |
| `invert_a` | Invert | `nw.audio.polarity.invert` | native | Invert audio polarity. |
| `loudness_radar` | Loudness Meter | `nw.audio.analysis.loudness_meter` | native | Measure loudness and peaks at the selected insert position without changing signal. |
| `mastering` | Mastering | `nw.audio.preset.mastering` | composed | Provide EQ, reverb, excitation, stereo width, limiting/soft clipping and output gain in order. |
| `panner_ambisonics` | Panner - Ambisonics | `nw.audio.spatial.virtual_stereo_pan` | native | Rotate two virtual stereo source directions and render their pan/elevation gains. |
| `swap_channels` | Swap Channels | `nw.audio.channels.swap_stereo` | native | Swap left/right while preserving any additional channels. |
| `vocal_enhancer` | Vocal Enhancer | `nw.audio.preset.vocal_voicing` | composed | Select voice or music voicings using EQ and gentle compression. |
| `stereo_expander` | Stereo Expander | `nw.audio.stereo.expand_pan` | native | Scale the side signal and pan the mid signal independently. |
| `stereo_width` | Stereo Width | `nw.audio.stereo.width` | native | Adjust mid/side width. |
| `pitch_shifter` | Pitch Shifter | `nw.audio.pitch.shift` | component | Shift pitch independently of duration with native quality/formant controls. |
| `balance_a` | Balance | `nw.audio.stereo.balance` | native | Balance left and right channels with a declared law. |
| `mute` | Mute | `nw.audio.mute` | native | Mute/unmute with a short de-click ramp. |
| `volume_a` | Volume | `nw.audio.gain_insert` | native | Apply bypassable gain at an inserted effect position. |
| `lighting_effects` | Lighting Effects | `nw.video.light.surface_lighting` | composed | Shade a 2D image surface using up to five directional/point/spot lights, optional luminance bump and ambient/material controls. |
| `vr_blur` | VR Blur | `nw.video.vr.blur` | composed | Blur a spherical image with seam-safe angular neighborhoods, per eye. |
| `vr_chromatic_aberrations` | VR Chromatic Aberrations | `nw.video.vr.chromatic_aberration` | composed | Displace RGB sample directions around a spherical center with adjustable falloff. |
| `vr_color_gradients` | VR Color Gradients | `nw.video.vr.color_field` | composed | Blend authored color points by spherical distance. |
| `vr_denoise` | VR De-Noise | `nw.video.vr.denoise` | composed | Reduce spatial noise on a spherical frame while preserving strong edges. |
| `vr_digital_glitch` | VR Digital Glitch | `nw.video.vr.glitch` | composed | Create wrapping band shifts, RGB offsets and scanline attenuation per eye. |
| `vr_fractal_noise` | VR Fractal Noise | `nw.video.vr.fractal_noise` | composed | Generate a seam-safe evolving octave noise field on a sphere and composite it. |
| `vr_glow` | VR Glow | `nw.video.vr.glow` | composed | Add thresholded highlight glow using seam-safe angular blur. |
| `vr_plane_to_sphere` | VR Plane to Sphere | `nw.video.vr.plane_projection` | native | Project a planar picture into an equirectangular sphere with transparent uncovered areas. |
| `vr_projection` | VR Projection | `nw.video.vr.fov_projection` | native | Rotate an equirectangular view and apply an explicit field-of-view/crop/stretch policy. |
| `vr_rotate_sphere` | VR Rotate Sphere | `nw.video.vr.rotate` | native | Rotate a full spherical picture without changing its declared projection/layout. |
| `vr_sharpen` | VR Sharpen | `nw.video.vr.sharpen` | composed | Sharpen a spherical picture using a seam-safe blur residual. |
| `bokeh_blur` | Bokeh Blur | `nw.video.blur.aperture` | composed | Blur through a circular/polygon aperture, with optional bright-highlight enhancement. |
| `channel_blur` | Channel Blur | `nw.video.blur.channels` | composed | Blur R, G, B and alpha by independently chosen amounts. |
| `compound_blur` | Compound Blur | `nw.video.blur.radius_map` | composed | Vary blur radius using another layer or the current picture luminance. |
| `focus_blur` | Focus Blur | `nw.video.blur.focus_region` | composed | Keep a radial/linear region sharp and blur progressively outside it. |
| `reduce_interlace_flicker` | Reduce Interlace Flicker | `nw.video.blur.vertical_soften` | native | Reduce line flicker through vertical-only softening. |
| `asc_cdl` | ASC CDL | `nw.video.color.asc_cdl` | composed | Apply slope, offset, power and saturation color correction. |
| `video_limiter` | Video Limiter | `nw.video.color.signal_limiter` | composed | Limit RGB and/or luma to declared output bounds, with soft compression and warning view. |
| `vignette` | Vignette | `nw.video.light.vignette` | composed | Apply a feathered elliptical/rounded light/dark border treatment. |
| `corner_pin` | Corner Pin | `nw.video.geometry.corner_pin` | composed | Map four source corners to an authored quadrilateral. |
| `magnify` | Magnify | `nw.video.geometry.loupe` | composed | Magnify a circular or rectangular region and composite it over the original. |
| `spherize` | Spherize | `nw.video.geometry.spherize` | composed | Magnify/warp the image inside a radial lens region. |
| `turbulent_displace` | Turbulent Displace | `nw.video.geometry.turbulence` | composed | Displace coordinates with an evolving multi-octave noise field. |
| `warp_stabilizer` | Warp Stabilizer | `nw.video.stabilize` | component | Analyze camera shake and smooth the picture transform in a two-pass stabilization workflow. |
| `gradient` | Gradient | `nw.video.generate.gradient` | composed | Generate linear/radial/reflected/diamond color ramps with midpoint, optional dither and blend. |
| `channel_mix` | Channel Mix | `nw.video.color.channel_matrix` | composed | Apply an affine RGB channel matrix with optional monochrome output. |
| `color_replace` | Color Replace | `nw.video.color.rgb_replacement` | composed | Select an RGB-distance neighborhood and replace its color or selected hue/saturation components. |
| `rounded_crop` | Rounded Crop | `nw.video.mask.rounded_crop` | composed | Mask to a rounded rectangle with feather and optional inner border. |
| `alpha_adjust` | Alpha Adjust | `nw.video.alpha.adjust` | composed | Change, replace, invert or inspect alpha while maintaining consistent color representation. |
| `logo_cutout` | Logo Cutout | `nw.video.key.flat_background` | composed | Remove a known flat background and optionally un-composite its color from edge pixels. |
| `echo_glow` | Echo Glow | `nw.video.light.echo_glow` | composed | Combine progressively wider highlight glows with geometric decay. |
| `edge_glow` | Edge Glow | `nw.video.light.edge_glow` | composed | Turn image edges into colored blurred light. |
| `glint` | Glint | `nw.video.light.glint` | composed | Create star-shaped streaks from bright image regions. |
| `light_leaks` | Light Leaks | `nw.video.light.leaks` | composed | Overlay animated colored light washes from seeded gradient blobs. |
| `rgb_split` | RGB Split | `nw.video.color.channel_offset` | composed | Offset RGB channels linearly or radially around a center. |
| `volumetric_rays` | Volumetric Rays | `nw.video.light.radial_rays` | composed | Create finite radial highlight streaks from an authored point. |
| `wonder_glow` | Wonder Glow | `nw.video.light.multiscale_glow` | composed | Combine several highlight blur radii with tint and saturation controls. |
| `long_shadow` | Long Shadow | `nw.video.shadow.long` | composed | Extrude visible alpha along a direction with optional fade. |
| `brush_strokes` | Brush Strokes | `nw.video.stylize.brush_strokes` | composed | Create a painterly look using short directional gathers with per-cell variation. |
| `color_emboss` | Color Emboss | `nw.video.relief.color_emboss` | composed | Add directional relief while retaining source color. |
| `roughen_edges` | Roughen Edges | `nw.video.mask.roughen` | composed | Perturb/erode alpha edges with procedural noise and selectable edge looks. |
| `posterize_time` | Posterize Time | `nw.video.time.hold_rate` | composed | Hold the preceding processed layer at quantized clip-relative sample times while preserving output cadence and clip duration. |
| `rotate_3d` | 3D Rotate | `nw.video.geometry.planar_xyz` | composed | Rotate one planar layer around X/Y/Z with perspective and depth. |
| `auto_reframe` | Auto Reframe | `nw.video.analysis.auto_reframe` | composed | Follow a gradient-energy centroid with center bias and temporal smoothing, then crop/scale to a target aspect. |
| `camera_shake` | Camera Shake | `nw.video.motion.camera_shake` | composed | Animate position, rotation and zoom from seeded smooth control noise. |
| `grow` | Grow | `nw.video.motion.grow` | composed | Animate scale between endpoints over clip time with selectable easing. |
| `shrink` | Shrink | `nw.video.motion.shrink` | composed | Animate scale downward or between authored endpoints over clip time. |
| `move` | Move | `nw.video.motion.move` | composed | Animate translation between authored endpoints. |
| `spin` | Spin | `nw.video.motion.spin` | composed | Animate rotation around an anchor. |
| `spacer` | Spacer | `nw.video.layout.spacer` | composed | Fit the image inside margins with rounded corners and optional background fill. |
| `wiggle` | Wiggle | `nw.video.motion.wiggle` | composed | Apply seeded smooth position/rotation/scale jitter. |
| `auto_align` | Auto Align | `nw.video.analysis.alpha_align` | composed | Find visible alpha bounds and align them to a chosen canvas edge or center. |
| `cineon_converter` | Cineon Converter | `nw.video.color.cineon` | composed | Convert a declared Cineon log encoding to/from linear light. |
| `clone` | Clone | `nw.video.tile.clone` | composed | Repeat a fitted picture in a rows/columns grid with gaps and alternating mirror options. |
| `stroke` | Stroke | `nw.video.alpha.outline` | composed | Outline the visible alpha inside, outside or centered on the boundary. |
| `metadata_burnin` | Metadata & Timecode Burn-in | `nw.video.text.burnin` | component | Burn text or resolved media/sequence metadata into the picture. |
| `echo` | Echo | `nw.video.time.echo` | composed | Composite finite earlier/later samples of the preceding processed layer in clip time, with decay and a chosen blend operator. |
| `lightning` | Lightning | `nw.video.generate.lightning` | component | Generate animated branched lightning paths with core and glow. |
| `cell_pattern` | Cell Pattern | `nw.video.generate.cellular_noise` | composed | Generate seeded cellular/Worley-style patterns from bounded neighboring sites. |
| `checkerboard` | Checkerboard | `nw.video.generate.checkerboard` | component | Draw antialiased alternating checker cells with color/transparent choices. |
| `ellipse` | Ellipse | `nw.video.generate.ellipse_ring` | component | Draw an elliptical ring with independently styled interior. |
| `paint_bucket` | Paint Bucket | `nw.video.fill.connected` | composed | Fill the connected component around an authored seed under a color/alpha tolerance. |
| `write_on` | Write-on | `nw.video.paint.write_on` | component | Accumulate an animated brush path up to the current time. |
| `alpha_glow` | Alpha Glow | `nw.video.light.alpha_glow` | composed | Glow around alpha edges with an inner-to-outer color treatment. |
| `block_dissolve` | Block Dissolve | `nw.video.mask.block_reveal` | composed | Reveal/hide a layer in seeded block order. |
| `directional_blur_legacy` | Directional Blur (Legacy) | `nw.video.blur.directional` | native | Blur along an authored direction. |
| `gaussian_blur_legacy` | Gaussian Blur (Legacy) | `nw.video.blur.gaussian` | native | Apply Gaussian blur with independent horizontal/vertical amount and declared edge treatment. |
| `gradient_wipe_legacy` | Gradient Wipe (Legacy) | `nw.video.mask.luma_wipe` | composed | Remove alpha according to a track/source luma field and completion. |
| `linear_wipe_legacy` | Linear Wipe (Legacy) | `nw.video.mask.linear_wipe` | composed | Remove alpha behind an angled sweeping boundary. |
| `magnify_legacy` | Magnify (Legacy) | `nw.video.geometry.loupe` | composed | Magnify a circular or rectangular region and composite it over the original. |
| `mosaic_legacy` | Mosaic (Legacy) | `nw.video.pixelate` | composed | Replace the image with a grid of reduced-resolution color blocks. |
| `noise_legacy` | Noise (Legacy) | `nw.video.noise.grain` | composed | Add mono or color grain with explicit scale, seed and clipping policy. |
| `twirl_legacy` | Twirl (Legacy) | `nw.video.geometry.twirl` | composed | Rotate sampling coordinates by a radial falloff around a center. |
| `block_motion` | Block Motion | `nw.transition.cards.block_motion` | composed | Animate a grid of image cards with staggered ordering. |
| `flip_motion` | Flip Motion | `nw.transition.cards.flip` | composed | Flip a planar picture card to reveal the other side. |
| `fold_motion` | Fold Motion | `nw.transition.cards.fold` | composed | Fold two hinged picture halves to reveal the next image. |
| `pop_motion` | Pop Motion | `nw.transition.cards.pop` | composed | Pop the incoming picture into place with overshoot. |
| `pull_motion` | Pull Motion | `nw.transition.cards.pull` | composed | Pull an incoming/outgoing picture through scale/depth motion. |
| `spin_motion` | Spin Motion | `nw.transition.cards.spin` | composed | Spin the image card around an authored center during replacement. |
| `spring_motion` | Spring Motion | `nw.transition.motion.spring` | composed | Move the incoming image into place with a damped spring trajectory. |
| `travel_motion` | Travel Motion | `nw.transition.cards.travel` | composed | Move a planar picture through an authored perspective trajectory. |
| `additive_dissolve` | Additive Dissolve | `nw.transition.dissolve.additive` | composed | Overlap images through an additive-light dissolve. |
| `blur_dissolve` | Blur Dissolve | `nw.transition.dissolve.blur` | composed | Blur both images around the transition midpoint while crossfading. |
| `burn_alpha` | Burn Alpha | `nw.transition.dissolve.alpha_burn` | composed | Burn away outgoing coverage with a shaped luma/color mask. |
| `cross_dissolve` | Cross Dissolve | `nw.transition.dissolve.cross` | composed | Crossfade the two images. |
| `dip_to_black` | Dip to Black | `nw.transition.dip.black` | composed | Fade outgoing to black then black to incoming. |
| `dip_to_color` | Dip to Color | `nw.transition.dip.color` | composed | Fade through an authored opaque color. |
| `dip_to_white` | Dip to White | `nw.transition.dip.white` | composed | Fade outgoing to white then white to incoming. |
| `film_dissolve` | Film Dissolve | `nw.transition.dissolve.perceptual` | composed | Blend in a declared nonlinear/gamma-adjusted domain for a film-like dissolve. |
| `luma_fade` | Luma Fade | `nw.transition.dissolve.luma` | composed | Reveal incoming through a soft luminance-driven threshold. |
| `morph_cut` | Morph Cut | `nw.transition.flow.morph` | composed | Interpolate motion between the corresponding outgoing/incoming transition frames. |
| `mosaic_transition` | Mosaic | `nw.transition.look.mosaic` | composed | Increase/decrease pixelation around an A/B dissolve. |
| `chaos` | Chaos | `nw.transition.look.chaos` | composed | Add seeded staggered movement/distortion around the transition. |
| `earthquake` | Earthquake | `nw.transition.look.earthquake` | composed | Shake, rotate and zoom the picture near the transition midpoint. |
| `flicker` | Flicker | `nw.transition.look.flicker` | composed | Modulate brightness while changing between images. |
| `glass` | Glass | `nw.transition.look.glass` | composed | Refract the picture through a procedural glass-cell field. |
| `glitch` | Glitch | `nw.transition.look.glitch` | composed | Use displaced horizontal bands and RGB separation during the cut. |
| `grunge` | Grunge | `nw.transition.look.grunge` | composed | Reveal the next picture through a distressed procedural mask. |
| `kaleidoscope` | Kaleidoscope | `nw.transition.look.kaleidoscope` | composed | Fold angular sectors into a mirrored kaleidoscopic transition. |
| `liquid_distortion` | Liquid Distortion | `nw.transition.look.liquid` | composed | Apply an animated smooth liquid-like coordinate field while crossfading. |
| `tv_power` | TV Power | `nw.transition.look.tv_power` | composed | Contract/expand the picture with a TV-switch light line/flash. |
| `vhs_damage` | VHS Damage | `nw.transition.look.vhs_damage` | composed | Combine horizontal tearing, chroma offsets, grain and scanline treatment. |
| `vr_chroma_leaks` | VR Chroma Leaks | `nw.transition.vr.light_leaks` | composed | Overlay wrapping colored light blobs during an A/B dissolve. |
| `vr_gradient_wipe` | VR Gradient Wipe | `nw.transition.vr.gradient_wipe` | composed | Reveal the next sphere with a vertical gradient and periodic longitude variation. |
| `vr_iris_wipe` | VR Iris Wipe | `nw.transition.vr.iris` | composed | Reveal the next image by angular distance from a spherical center. |
| `vr_light_leaks` | VR Light Leaks | `nw.transition.vr.light_leaks` | composed | Overlay wrapping colored light blobs during an A/B dissolve. |
| `vr_light_rays` | VR Light Rays | `nw.transition.vr.light_rays` | composed | Add wrapping angular light rays around the midpoint dissolve. |
| `vr_mobius_zoom` | VR Mobius Zoom | `nw.transition.vr.zoom_twist` | composed | Apply a longitude-wrapping radial zoom/twist during transition. |
| `vr_random_blocks` | VR Random Blocks | `nw.transition.vr.blocks` | composed | Reveal by seeded horizontal-wrapping block order. |
| `vr_spherical_blur` | VR Spherical Blur | `nw.transition.vr.blur_dissolve` | composed | Blur an A/B mix with the source feature’s flat approximation, optionally choose the spherical blur mode. |
| `burn_chroma` | Burn Chroma | `nw.transition.look.chroma_burn` | composed | Add chromatic light around the midpoint blend. |
| `chroma_leak` | Chroma Leak | `nw.transition.look.light_leak` | composed | Wash the midpoint blend with moving colored gradient blobs. |
| `cross_zoom` | Cross Zoom | `nw.transition.blur.zoom` | composed | Blur radially by scaling samples about a center while crossfading. |
| `directional_blur_transition` | Directional Blur | `nw.transition.blur.directional` | composed | Apply directional blur around the midpoint crossfade. |
| `flare` | Flare | `nw.transition.light.flare` | composed | Add an authored light core, halo and streak during the cut. |
| `flash` | Flash | `nw.transition.light.flash` | composed | Flash an authored color over the transition. |
| `glow` | Glow | `nw.transition.light.glow` | composed | Add thresholded blurred light around the transition midpoint. |
| `lens_blur` | Lens Blur | `nw.transition.blur.aperture` | composed | Apply aperture blur to transition inputs under a midpoint envelope, then crossfade. |
| `light_leak` | Light Leak | `nw.transition.look.light_leak` | composed | Wash the midpoint blend with moving colored gradient blobs. |
| `light_sweep` | Light Sweep | `nw.transition.light.sweep` | composed | Sweep a moving band of light across the images. |
| `phosphor` | Phosphor | `nw.transition.look.phosphor` | composed | Create a channel-biased glow/persistence-style cut. |
| `radial_blur` | Radial Blur | `nw.transition.blur.angular` | composed | Blur angularly around a center during the cut. |
| `ray` | Ray | `nw.transition.light.rays` | composed | Overlay radial shafts of light during the transition. |
| `solarize` | Solarize | `nw.transition.look.solarize` | composed | Apply a thresholded nonlinear color inversion during the blend. |
| `stripe` | Stripe | `nw.transition.light.stripes` | composed | Move bands of colored light across the A/B blend. |
| `zoom_blur` | Zoom Blur | `nw.transition.blur.zoom` | composed | Blur radially by scaling samples about a center while crossfading. |
| `roll_3d` | 3D Roll | `nw.transition.cards.roll_3d` | composed | Roll a planar picture card in perspective with front/back visibility. |
| `film_roll` | Film Roll | `nw.transition.motion.film_roll` | composed | Move/rotate pictures like frames on a film strip. |
| `push` | Push | `nw.transition.motion.push` | composed | Push outgoing and incoming images together across the frame. |
| `roll` | Roll | `nw.transition.motion.roll` | composed | Combine translation and rotation as pictures change. |
| `slide` | Slide | `nw.transition.motion.slide` | composed | Slide incoming over stationary outgoing image. |
| `split` | Split | `nw.transition.motion.split` | composed | Move or reveal outgoing halves away from a chosen split. |
| `stretch` | Stretch | `nw.transition.motion.stretch` | composed | Stretch and translate the pictures during replacement. |
| `whip` | Whip | `nw.transition.motion.whip` | composed | Perform a fast directional move with directional/motion blur. |
| `motion_camera` | Motion Camera | `nw.transition.motion.camera_style` | composed | Apply authored camera-like transforms to the two images. |
| `motion_tween` | Motion Tween | `nw.transition.motion.tween` | composed | Interpolate parameter-defined transforms between the two images. |
| `shape_dissolve` | Shape Dissolve | `nw.transition.mask.shape_cells` | composed | Reveal through a grid of growing circle/box/diamond/star shapes. |
| `shape_flow` | Shape Flow | `nw.transition.mask.shape_flow` | composed | Move and enlarge a shape from an edge to reveal the next image. |
| `text_animator` | Text Animator | `nw.transition.cells.staggered_slide` | composed | Slide rectangular image cells vertically with row/column staggering. |
| `typewriter` | Typewriter | `nw.transition.cells.reading_reveal` | composed | Reveal rectangular image cells in reading order, optionally with a caret block. |
| `spin_3d` | 3D Spin | `nw.transition.cards.spin_3d` | composed | Spin a textured planar card to reveal the next image. |
| `spinback_3d` | 3D Spinback | `nw.transition.cards.spinback` | composed | Spin the card away/back in depth while switching images. |
| `frame` | Frame | `nw.transition.cards.frame_parts` | composed | Separate frame-shaped picture elements to reveal the incoming image. |
| `louver` | Louver | `nw.transition.cards.louver` | composed | Rotate a set of hinged image slats. |
| `mirror_transition` | Mirror | `nw.transition.cards.mirror` | composed | Animate mirrored image-card arrangements during the transition. |
| `page_peel` | Page Peel | `nw.transition.page.peel` | composed | Peel a page corner over a curved roll, exposing a shaded back and incoming image. |
| `slice` | Slice | `nw.transition.cards.slices` | composed | Move picture slices with staggered timing and optional depth. |
| `wave` | Wave | `nw.transition.cards.wave` | composed | Animate picture sections along a wave of displacement/rotation. |
| `clock_wipe` | Clock Wipe | `nw.transition.wipe.clock` | composed | Sweep an angular reveal around an authored center. |
| `linear_wipe` | Linear Wipe | `nw.transition.wipe.linear` | composed | Sweep an angled boundary across the picture with adjustable feather. |
| `neon_wipe` | Neon Wipe | `nw.transition.wipe.neon` | composed | Reveal along a moving edge with colored core and glow. |
| `panel_wipe` | Panel Wipe | `nw.transition.wipe.panels` | composed | Reveal through direction-aligned strips with staggered delay. |
| `plateau_wipe` | Plateau Wipe | `nw.transition.wipe.plateau` | composed | Reveal with a stepped boundary and symmetric strip delays. |
| `radial_wipe` | Radial Wipe | `nw.transition.wipe.corner_angle` | composed | Sweep an angular reveal originating at a selected frame corner. |
| `soft_wipe` | Soft Wipe | `nw.transition.wipe.linear` | composed | Sweep an angled boundary across the picture with adjustable feather. |
| `star_wipe` | Star Wipe | `nw.transition.wipe.iris` | composed | Reveal through a growing rotated star shape. |
| `stretch_wipe` | Stretch Wipe | `nw.transition.wipe.stretch_edge` | composed | Reveal while stretching incoming content near the moving boundary. |
| `additive_dissolve_legacy` | Additive Dissolve (Legacy) | `nw.transition.dissolve.additive` | composed | Overlap images through an additive-light dissolve. |
| `barn_doors` | Barn Doors | `nw.transition.wipe.barn_doors` | composed | Reveal outward/inward from a central horizontal or vertical line. |
| `center_split` | Center Split | `nw.transition.motion.split` | composed | Move or reveal outgoing halves away from a chosen split. |
| `clock_wipe_legacy` | Clock Wipe (Legacy) | `nw.transition.wipe.clock` | composed | Sweep an angular reveal around an authored center. |
| `cross_dissolve_legacy` | Cross Dissolve (Legacy) | `nw.transition.dissolve.cross` | composed | Crossfade the two images. |
| `cross_zoom_legacy` | Cross Zoom (Legacy) | `nw.transition.motion.zoom_cross` | composed | Scale images around center while crossfading. |
| `dip_to_black_legacy` | Dip to Black (Legacy) | `nw.transition.dip.black` | composed | Fade outgoing to black then black to incoming. |
| `dip_to_white_legacy` | Dip to White (Legacy) | `nw.transition.dip.white` | composed | Fade outgoing to white then white to incoming. |
| `film_dissolve_legacy` | Film Dissolve (Legacy) | `nw.transition.dissolve.perceptual` | composed | Blend in a declared nonlinear/gamma-adjusted domain for a film-like dissolve. |
| `inset` | Inset | `nw.transition.wipe.corner_box` | composed | Reveal with an expanding aspect-correct rectangular metric from a corner. |
| `iris_box` | Iris Box | `nw.transition.wipe.iris` | composed | Reveal with a growing rectangle around a center. |
| `iris_cross` | Iris Cross | `nw.transition.wipe.iris` | composed | Reveal with expanding crossed bars. |
| `iris_diamond` | Iris Diamond | `nw.transition.wipe.iris` | composed | Reveal with an expanding diamond. |
| `iris_round` | Iris Round | `nw.transition.wipe.iris` | composed | Reveal with an expanding circle/ellipse. |
| `non_additive_dissolve` | Non-Additive Dissolve | `nw.transition.dissolve.non_additive` | composed | Use a luminance-dependent nonlinear dissolve. |
| `push_legacy` | Push (Legacy) | `nw.transition.motion.push` | composed | Push outgoing and incoming images together across the frame. |
| `radial_wipe_legacy` | Radial Wipe (Legacy) | `nw.transition.wipe.clock` | composed | Sweep an angular reveal around an authored center. |
| `slide_legacy` | Slide (Legacy) | `nw.transition.motion.slide` | composed | Slide incoming over stationary outgoing image. |
| `split_legacy` | Split (Legacy) | `nw.transition.motion.split` | composed | Move or reveal outgoing halves away from a chosen split. |
| `whip_legacy` | Whip (Legacy) | `nw.transition.motion.whip` | composed | Perform a fast directional move with directional/motion blur. |
| `wipe` | Wipe | `nw.transition.wipe.linear` | composed | Sweep an angled boundary across the picture with adjustable feather. |
| `band_slide` | Band Slide | `nw.transition.motion.band_slide` | composed | Slide horizontal picture bands with staggered timing. |
| `checker_wipe` | Checker Wipe | `nw.transition.wipe.checker` | composed | Reveal alternating checker cells. |
| `cube_spin` | Cube Spin | `nw.transition.motion.cube_style` | composed | Create the source feature’s cube-like scale/swap transition. |
| `flip_over` | Flip Over | `nw.transition.motion.flip_style` | composed | Collapse and expand image scale around a flip point. |
| `gradient_wipe` | Gradient Wipe | `nw.transition.wipe.gradient` | composed | Reveal through a procedural or supplied luminance-gradient field. |
| `page_turn` | Page Turn | `nw.transition.page.turn` | composed | Turn a page using a diagonal mask/warp with shading. |
| `venetian_blinds` | Venetian Blinds | `nw.transition.wipe.blinds` | composed | Reveal in repeated horizontal or vertical strips. |
| `graphic_text` | Text | `nw.graphic.text_layer` | component | Render structured rich text layers with per-run styles, paragraph/vertical layout, fills, two strokes, background, shadow and transforms. |
| `graphic_shape` | Shape | `nw.graphic.shape_layer` | component | Render rectangle, ellipse, polygon and authored path layers with fills/gradients, multiple strokes, background, shadow and transforms. |

### EffectCraft decisions

The tables below use the reconciled canonical identities. Per-donor reports retain their original review proposals as provenance; where an ID differs, the combined ledger and explicit alias/variant rules are authoritative. They do not discard the per-row semantic/resource restrictions.

This report and its companion CSV replace the EffectCraft half of the v4 candidate scaffold. Every one of the 307 pinned source entries now has an individual functional design disposition and a semantic capability ID. **No row is an implemented, executed or performance-qualified Numera capability yet.** Completion of this mapping is design/research completion, not 307 working effects.

The source inventory is [EffectCraft 9db7f3b](https://github.com/storytold/effectcraft/tree/9db7f3b21700fd821f215782ed1c82afe077880f). The FFmpeg route evidence is the [pinned FFmpeg filter manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi). Exact source-definition, implementation and filter-section links are retained per CSV row. Clean evaluated source descriptors are provided in `research_v7/effectcraft-source-descriptors.json`; no superseded algorithm-identity catalogue or recipe is required by this handoff.

#### 1. Review result and what the labels mean

| Design disposition | Source entries | Meaning |
|---|---:|---|
| `native` | 45 | An actual documented FFmpeg processor supplies the meaningful task; small setup/composition helpers may surround it. |
| `composed` | 211 | A specific bounded graph of documented FFmpeg operations supplies the defined task. |
| `component` | 21 | A documented FFmpeg route has an explicit dependency/resource qualification. This label does not mean optional: ordinary Cairo/text/libvidstab/Rubber Band functions use the standard portable profile; specialized OpenColorIO or IR assets may use declared optional profiles or user assets. |
| `application` | 11 | Ordinary typed project/control data. It has no media rendering semantics and must not become a fake effect. |
| `unsupported` | 19 | The primary semantic feature has no practical admitted FFmpeg route under the current boundary; useful alternatives are named separately. |

The individual donor review produced **296 semantic proposals** across 307 source records, before the canonical cross-donor reconciliation above. Intentional deduplication includes legacy/modern Gaussian and median blur, radial-blur variants, levels presentations, multitone presets, related chroma-key workflows and control variants. This is a new product schema, not a donor-ID compatibility layer.

| Functional coverage | Source entries | Interpretation |
|---|---:|---|
| `core` | 236 | The meaningful user task has a defined route. Canonical parameter names/units and algorithms may differ from the donor; this does not claim every donor option or exact output. |
| `partial` | 41 | A specifically stated useful part is admitted; the CSV explicitly describes the remaining semantic limitation. |
| `control` | 11 | The source entry is a pass-through typed data/control record. |
| `none` | 19 | The primary feature is unsupported; separately named alternatives do not count as primary support. |

**`target_capability_id` identifies a processor, generator, preset family or typed record. `effects.add`, `effects.update` and `effects.remove` are action commands that operate on admitted processors. They are not effect identities.** Application records use the ordinary project parameter/edit schema; unsupported primary processors expose a reason through capability discovery and are not advertised as runnable effects. The core command registry owns action naming for project data.

All semantic capability IDs and separately named alternatives use the common `nw.*` namespace. The CSV retains `original_proposed_capability_id` solely as review provenance; it is not a second API alias. Cross-donor semantic deduplication must preserve one canonical processor descriptor when the defined behavior is the same.

#### 2. Functional corrections that materially change the project scope

The bounded v7 temporal correction makes `ec.time.echo` sample the preceding processed layer at explicit clip-clock offsets, and `ec.time.posterizetime` hold that layer at quantized clip-relative times without changing output cadence or duration. Prior effects remain in the sampled input; animated offsets/rates need an admitted bounded sample schedule. The existing Posterize Time proposal still resolves through the alias ledger to `nw.video.time.hold_rate`; this is one canonical implementation, not another effect or an ID migration in the source ledger. IDs, counts, dispositions, filter lists and unqualified runtime status remain unchanged. [Pinned temporal frame access](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/time_fx.rs#L159-L220) · [Pinned Echo and Posterize Time](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/time_fx.rs#L264-L342)

The v5 media correction aligns `ec.stylize.coloremboss`, `ec.generate.fourcolor` and `ec.generate.gradientramp` with their shared canonical FilmCraft descriptors. Color relief requires a finite directional neighborhood operation with a signed residual and original-color/alpha handling. Four-point and shaped gradients require a finite spatial field with explicit coordinates, time, degeneracy and optional deterministic dither. Native gradient modes are admitted only when their actual geometry matches; other already-defined shapes use a compact coordinate expression. The updated CSV gives the shared routes and focused CI fixtures. IDs, dispositions and runtime status remain unchanged; this reconciliation does not assert that the graphs have been implemented or executed.

The bounded color-family followup also reconciled `ec.blur.unsharp`, `ec.stylize.findedges`, `ec.stylize.emboss`, `ec.stylize.strobe` and `ec.channel.invert` with their shared canonical routes and focused CI fixtures. Spatial residuals/edges require their actual neighboring samples and intermediate buffers; strobing requires clip-time gating with declared units and stable seek semantics; alpha inversion requires actual alpha processing. Native zero-threshold unsharp is a shortcut only for a matching kernel/precision contract. Canny and Sobel remain distinct detector variants. `ec.color.brightnesscontrast` no longer invents an unrelated optional gamma control from the native processor's larger option set. This followup inspected the 23 EffectCraft rows related to the remaining 22 FilmCraft color-family rows and did not broaden donor scope or change any IDs/dispositions.

The earlier output over-constrained ordinary effects by making exact donor mathematics a prerequisite. That is removed. Gaussian blur uses `gblur`; bilateral uses `bilateral`; threshold-aware smoothing uses `smartblur`; equalization uses `histeq`; selective CMYK correction uses `selectivecolor`; grain/denoise use native FFmpeg processors; compression, gating, chorus, modulation and distortion use the corresponding audio filters. Source units are migrated to truthful native controls rather than being copied into unrelated FFmpeg parameters.

Several specific omissions are now closed:

- **Leave Color:** `colorhold` provides native RGB selective color retention; `hsvhold` supplies hue-domain selection. Tolerance/softness use the chosen native metric. This is not an unsupported effect because the donor used a different color-distance formula. [FFmpeg colorhold](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L10025-L10049) · [hsvhold](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15893-L15930)
- **Depth/map blur:** `varblur` consumes a same-size, same-layout guide and reads the corresponding guide plane for each processed picture plane. Float RGB therefore uses a scalar replicated into GBRPF32/GBRAPF32 planes; a generic GRAYF32 guide is not admitted. The pinned transfer is not a literal zero-to-R pixel-radius mapping, and `min_r=0` still filters. Calibrate native controls from the source, preserve zero-blur pixels through an original-source mask bypass and qualify impulse/edge behavior. Charge summed-area tables for every plane, including disabled ones. This supplies useful depth/map defocus without rebuilding the donor five-level blur math; aperture-shaped lens blur remains a separate convolution recipe. [varblur manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L25137-L25157) · [guide/transfer source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_varblur.c#L75-L245) · [storage](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_varblur.c#L326-L359)
- **Matte refinement:** `guided` supports a genuine second guidance input, so edge-aware refinement can be composed with alpha morphology/contrast/feather. This provides useful soft/hard matte refinement without pretending that generic erosion alone performs segmentation. [guided](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15439-L15492)
- **OpenColorIO:** the pinned FFmpeg manual contains a real `ocio` filter with `config`, `input`, `output`, `display`, `view`, `inverse`, `filetransform`, RGB output format and context parameters. Space/display/file transforms therefore have actual component-qualified routes. The wrapper has no direct `looks` option: a look must be encoded in a prepared configuration/view or file transform. [ocio](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L19013-L19077)
- **Noise textures:** native `perlin` supports octave/persistence, spatial/time scales and seeds. Native noise fields plus short masks/transforms are legitimate fractal/turbulent visual functions. Rebuilding the donor RNG/interpolation engine is unnecessary. [perlin](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L30470-L30540)
- **Text/vector drawing:** `drawtext`/libass and `drawvg` are real FFmpeg routes. Ordinary paths, shape controls, glyph outlines and keyframes may be prepared as data in Rust, while FFmpeg performs rasterization. Rasterize geometry/coverage on separate Cairo surfaces with their actual supported formats and premultiplied-alpha contract, then convert/composite into the declared working domain; do not quantize the entire HDR/float base through drawvg. Cairo is part of the standard profile for these ordinary features. [drawvg manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L13236-L13308) · [format/alpha source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_drawvg.c#L2675-L2694)
- **Transitions and geometry:** native `xfade`, `perspective`, `v360`, `displace`, `remap` and compact analytic maps cover common transitions, 360 projections, corner pins, lens/warp effects and tiling. Short controlled compositions are supportable designs; a same-named donor shader is not required. [xfade](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L26276-L26414) · [v360](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L24660-L25073)

These are design inferences from documented primitives and source intent. They still need compiled-profile availability, correct graph wiring, visual/audio acceptance fixtures and measured resource qualification in GitHub CI. A listed filter does not prove that every format, runtime command, hardware suffix or alpha mode will work.

The bounded v5 light/noise review also corrected the shared turbulence, cellular-pattern, vignette and sample-noise routes. Their actual vector sampling, F1/F2 site search, signed border interpolation and residual composition are specified in the CSV. Preserve the separate noise overflow variants: EffectCraft `clip=false` wraps modulo one, whereas FilmCraft preserves headroom. Native `noise` is only a fast path for the modes it actually implements. IDs, design dispositions and runtime status remain unchanged; the new source-informed routes require CI qualification. See `research_v5/light-noise-review.md` in the companion.

#### 3. Processing boundary and practical limits

Rust owns project data, UI, parameter validation, geometry/control compilation, scheduling, process supervision and parsing of numerical FFmpeg analysis results. FFmpeg owns decoding, sampling pixels/audio, filtering, rasterization, media generation, compositing and encoding. Preparing a vector path is ordinary control work. Implementing a particle/physics/3D simulation engine and feeding its rendered state to FFmpeg does not satisfy this mandate.

Prefer an efficient native filter over a generic expression graph. Compact `geq` expressions are justified for simple transfer functions, masks and analytic inverse maps. They are not permission to unroll a donor renderer, a PDE solver, iterative optical-flow search, thousands of particles or one primitive per source pixel. The pinned `geq` source lists floating planar RGB/alpha and gray formats, but every surrounding stage still needs a negotiated format/alpha contract. [geq format implementation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/libavfilter/vf_geq.c#L340-L377)

The numerical limits in the CSV are **proposed conservative admission limits, not performance results**. They deliberately constrain the initial recipes: small spatial tap counts, usually 16 or fewer geometric patches, a few temporal branches, bounded native windows, bounded vector primitives and explicit source duration. They may only be changed after CI/resource evidence. A budget rejection must explain the exceeded limit and offer an explicit lower-quality/disk-backed alternative; never silently drop particles, samples, source options or frames.

Budget decoded frame queues, format conversion, GPU transfers, temporary filter buffers and output-size growth, not merely the Rust process RSS. `areverse`, large temporal offsets, motion interpolation, FFT convolution, guided filtering and flood fill need explicit admission. The source-reviewed floodfill queue can itself be substantial; a 4K/8K job cannot be accepted on the basis that the visible mask is only one channel. A long reverse can use bounded disk-backed segments with reversed segment order; a live infinite source cannot be reversed.

For spherical effects, isolate stereo eyes. A padded cubemap processing recipe is an explicit approximation that introduces reprojection. Padding must cover filter support and seam/pole fixtures must pass. A planar blur on an equirectangular frame is not automatically a VR blur. Likewise, do not use the `tile` filter to repeat an animated texture: it arranges successive frames; periodic coordinate mapping expresses actual spatial repetition.

A flat 2D rain/snow/bubble/lightning template is allowed when its function and limitations are explicit. It is not a license to advertise donor particle simulation, interacting foam, source-derived hair, physical shatter or 3D card dynamics as complete. Those primary features are accounted for below.

#### 4. Precisely unsupported primary features

The following decisions mean “no practical documented/admitted route under this scope at this pin,” not a theorem that no future FFmpeg component could implement the feature. Do not block ordinary supported effects while researching these; do not silently count their alternatives as support.

| Source feature | Unsupported primary semantics and admitted alternative |
|---|---|
| [Cryptomatte](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/channel3d.rs#L539) | Name/wildcard selection requires manifest decoding, raw float-bit object hashes and ranked coverage aggregation. A numeric RGB key or imported alpha matte does not supply that semantic feature. No admitted route for native Cryptomatte manifest/rank selection. Separate alternative nw.video.matte.apply uses alphamerge or maskedmerge with an ordinary supplied matte. |
| [Camera-Shake Deblur](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/blur3.rs#L259) | Donor uses sharper neighboring patches and alignment, not a generic sharpening pass. Known-kernel deconvolution is a useful explicit alternative, not automatic shake recovery. No practical admitted automatic temporal patch-selection/registration route. Separate nw.video.restore.psf_deblur can use deconvolve with a supplied blur kernel; blurdetect can identify blur. |
| [Puppet](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/puppet/mod.rs#L33) | Automatic alpha-derived triangulation, pin constraints and as-rigid-as-possible solving are the donor's primary function; importing finished vertices is only a separate alternative. No admitted FFmpeg pin/ARAP deformation solver. Alternative nw.video.warp.mesh applies explicit deformed 2D vertices through FFmpeg. |
| [Rolling Shutter Repair](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/distort4.rs#L599) | Rolling-shutter repair requires estimating time-varying row motion; global vidstab/deshake does not implement that feature. No admitted automatic row-dependent motion-estimation/correction route. Alternative nw.video.warp.row_transform accepts supplied row-transform coefficients. |
| [Object Matte](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/object_matte.rs#L41) | Object prompting, graph-cut/MobileSAM selection and resegmented optical-flow tracking are the primary unsupported semantics. dnn_processing alone does not prove a promptable multi-input workflow. No admitted FFmpeg click/box segmentation plus temporal object-propagation pipeline. Separate nw.video.matte.apply imports a final matte and applies it. |
| [Roto Brush & Refine Edge](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/roto.rs#L44) | Automatic stroke-driven graph-cut/model segmentation, optical-flow propagation and freeze cache are unsupported as an integrated feature. Manual roto is a distinct supported workflow. No admitted FFmpeg brush-prompted segmentation and temporal propagation pipeline. Alternatives are manually keyed vector matte plus native matte refinement, or an imported final matte. |
| [3D Camera Tracker](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/camera_tracker.rs#L32) | Automatic 3D camera tracking, solve diagnostics and scene reconstruction are unsupported; imported camera data is project data, not a computed solve. No admitted FFmpeg feature tracking plus bundle/camera/lens solve. Separate operations can display imported tracks with drawvg or apply supplied lens coefficients. |
| [CC Ball Action](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim2.rs#L508) | The donor turns a sampled grid into depth-sorted projected balls, including source-driven twist. A flat ball mosaic is useful but is not the complete spatial animation feature. No admitted full source-colored 3D ball/scatter/twist simulation. Alternative nw.video.stylize.ball_mosaic uses pixelize plus a periodic circular mask and simple local shading. |
| [CC Hair](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim3.rs#L1145) | Density-driven roots from source alpha, hairfall-map growth, droop and Kajiya-Kay strand shading require the donor's custom strand engine; ordinary strokes are not that automatic feature. No admitted practical FFmpeg source-driven strand generation/shading system. Separate vector-stroke drawing can render authored strands. |
| [CC Mr. Mercury](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim.rs#L1591) | Birth/lifetime/velocity/gravity/resistance-driven liquid particle evolution is unsupported. A bounded manually animated blob graphic is a distinct partial alternative. No admitted interacting/simulated Mr. Mercury pipeline. Separate nw.video.generate.metaball_template may render a few user-keyed analytic blobs and simple refraction. |
| [CC Particle Systems II](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim.rs#L285) | Donor birth/lifetime, many physics modes, inheritance and particle-type behavior are a simulation system. Decorative confetti is not full particle-system support. No admitted full particle producer/physics/shape engine through FFmpeg. Separate nw.video.generate.confetti_template renders a bounded set of closed-form 2D trajectories. |
| [CC Particle World](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim.rs#L284) | Three-dimensional producers, forces, camera projection, depth sorting and shutter sampling are unsupported as a combined simulation feature. No admitted 3D particle-world simulation/camera/depth-sort processor. Separate flat particle/graphic templates may be rendered by FFmpeg. |
| [CC Pixel Polly](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim2.rs#L509) | Seeded force/gravity/spin, per-piece depth and textured occlusion are the unsupported primary semantics. A 16-tile 2D transition is a distinct alternative. No admitted full 3D pixel-fragment simulation. Separate nw.video.transition.tile_explode uses a small explicit 2D tile set with keyed transforms. |
| [CC Scatterize](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim2.rs#L510) | Forward scatter creates holes/overlaps and additive accumulation in source order; an inverse map is not equivalent to that primary behavior. No practical admitted forward source-pixel splatting/depth-twist path. Separate nw.video.stylize.scatter performs inverse random displacement or block shuffling. |
| [Card Dance](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim2.rs#L148) | Gradient-layer channels driving per-card 3D positions/rotations, scene camera and lighting are unsupported as one engine. No full image-driven 3D card-dance pipeline is admitted. Alternative nw.video.transition.tile_transform animates a small explicit tile grid with 2D projective transforms. |
| [Foam](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim2.rs#L2163) | Repulsion, stickiness, popping, flow-map forces, reflected textures and stateful seeking are unsupported simulation semantics. No admitted interacting foam/bubble simulation. Separate nw.video.generate.bubbles provides noninteracting 2D decoration. |
| [Particle Playground](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim3.rs#L1143) | Persistent/ephemeral image-driven properties, interacting forces, wall collisions, source-time layer maps and particle explosion are unsupported. No admitted Cannon/Grid/Exploder/force/property-map particle engine. Separate finite vector templates cover simple decorative emissions. |
| [Shatter](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim2.rs#L149) | Custom fracture maps, fixed regions, forces, extruded sides and scene material behavior are unsupported. No admitted fracture/extruded-piece/force/camera/occlusion simulation. Separate nw.video.transition.tile_explode supplies a small flat tile-break transition. |
| [Wave World](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/sim2.rs#L2078) | Wave World integrates a spatial PDE with damping, producers, boundaries and ground data. An animated texture is not that solver. No admitted stateful height/velocity wave-equation solver with ground and reflecting boundaries. Separate nw.video.generate.water_texture can use perlin or analytic ring waves. |

Partial rows also contain specific limits: arbitrary named auxiliary-channel discovery, direct OCIO look selectors, optional exact gate hold, analog broadcast-signal certification, automatic image-contour extraction, automatic grain-correlation recovery, dense temporal displacement, arbitrary 3D scene lights and high-density geometry. These are declared parameter/scope boundaries, not generic “investigate later” labels.

#### 5. Controls, data and misleading shortcuts to avoid

The nine Expression Controls are pass-through values, and Face Measurements/Face Track Points are stored tracking-result channels. Preserve them as typed project parameters with undo/keyframes/reference semantics. Do not require automatic face tracking to implement their data containers, and do not infer a face tracker merely because the containers exist. [Expression Controls source](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/controls.rs#L1-L52) · [Face records source](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/face_track.rs#L1-L45)

Camera-Shake Deblur is a revealing example: its source searches sharper neighboring patches, aligns them and feathers replacements. `deconvolve` with a supplied point-spread function is a useful different restoration operation, but it does not automatically recover that missing temporal analysis. [Source description](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/blur3.rs#L1-L13) · [FFmpeg deconvolve](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L11553-L11575)

HDR Compander is another: the source is signed gain/power and its inverse. It does not guarantee that all values become 0–1. Clipping/quantization between compression and expansion destroys reversibility. CC Overbrights is a range-warning visualization, not a tone mapper. These source semantics are reflected in separate canonical capabilities. [Compander source](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/utility2.rs#L14-L34) · [Overbrights/highlight source](https://github.com/storytold/effectcraft/blob/9db7f3b21700fd821f215782ed1c82afe077880f/crates/effects/src/utility.rs#L27-L82)

Never silently attach source parameters to an unrelated native option. Example migrations must spell out Hz versus Q/octaves, dB versus linear gain, pixels versus sigma, source-time versus composition-time, straight versus premultiplied alpha, float HDR versus integer code values, and point sampling versus block averaging. Native pixelation and curves may be excellent functional replacements while having different numerical output.

#### 6. Implementation and qualification obligation

For each admitted capability, implement one versioned descriptor with supported inputs, canonical controls/units, constraints, default working format, alpha model, output/timing contract, lowering strategy, component profile, resource estimate and error reasons. The GUI inspector, CLI flags/schema and MCP tool arguments consume the same descriptor. A new filter or preset extends this registry and its lowering strategy, not three separate frontends. The source IDs remain provenance unless a separately authorized import/compatibility feature needs them.

The registry must distinguish design-admitted, compiled/available, implemented and qualified states. Listing a processor or loading a descriptor is not support. Each release profile must prove its actual filters, options, pixel formats, dependency licenses and execution behavior. The standard portable FFmpeg profile bundles Cairo/pixman for drawvg, required text shaping/libass/font dependencies, zimg/zscale, libvidstab and Rubber Band for admitted ordinary features. Their absence is a packaging/release failure, not an acceptable optional-feature state; ordinary supported functions work offline without another installation.

Specialized OpenColorIO profiles, heavy models and licensed or user-supplied IR/HRTF assets may be optional under the main component policy. They require precise profile/asset requirements and a real offline install/import or user-asset contract. A base convolution processor can be present while a particular external IR is absent. Required standard components and optional packs are measured separately; neither category is preloaded into memory merely because it is installed.

Qualification belongs in GitHub CI: graph parsing and real execution on small fixtures; visible/audio semantic checks; alpha/color/PTS/cadence/edge/end-point tests; missing-input/component/error paths; safe argument/asset handling; cancellation and process-tree cleanup; memory/queue/performance bounds; deterministic seed tests; and generated GUI/CLI/MCP contract parity. Use meaningful fixtures for distinct risks rather than duplicating implementation-shaped tests. GPU execution remains on the owner-authorized local GPU system; CPU capability must work independently. This review ran no app builds, no FFmpeg jobs and no media tests.

When a recipe uses several inputs, explicitly synchronize dimensions, pixel formats, rate/timebase where required, source-time mapping, start/end padding and framesync EOF behavior. For `xfade`, both inputs must be constant-rate and share resolution, format, rate and timebase; zero-duration transition is a cut. For GPU routes, qualify actual filter/format support and transfers independently; never append a hardware suffix and claim equivalence.

The final plan must be concrete before execution: show the final FFmpeg argv/filtergraph and selected backend, component profile, expected duration, output paths and material approximation/resource choices. Keep machine transport stdout separate from diagnostics. Record the graph/version/assets/seed/time/color provenance so the same operation can be reproduced by human CLI, GUI or agent MCP.
#### Complete EffectCraft source index

| Source ID | Source label | Canonical capability | Design | Coverage | Native primitives / declared alternative |
|---|---|---|---|---|---|
| `ec.3d.channelextract` | 3D Channel Extract | nw.video.auxiliary.channel_extract | composed | partial | extractplanes,geq,mergeplanes |
| `ec.3d.cryptomatte` | Cryptomatte | nw.video.matte.cryptomatte_select | unsupported | none | alphamerge,maskedmerge |
| `ec.3d.depthmatte` | Depth Matte | nw.video.matte.depth | composed | core | geq,alphamerge |
| `ec.3d.depthoffield` | Depth of Field | nw.video.blur.depth_of_field | composed | core | geq,varblur,maskedmerge |
| `ec.3d.extractor` | EXtractoR | nw.video.auxiliary.channels_remap | composed | partial | extractplanes,mergeplanes,geq,unpremultiply |
| `ec.3d.fog3d` | Fog 3D | nw.video.composite.depth_fog | composed | core | geq,color,maskedmerge |
| `ec.3d.idmatte` | ID Matte | nw.video.matte.numeric_id | composed | core | geq,gblur,alphamerge |
| `ec.3d.identifier` | IDentifier | nw.video.auxiliary.id_visualize | composed | core | geq,lutrgb,alphamerge |
| `ec.audio.backwards` | Backwards | nw.audio.time.reverse | native | core | atrim,areverse,asetpts,pan |
| `ec.audio.basstreble` | Bass & Treble | nw.audio.eq.bass_treble | composed | core | bass,treble |
| `ec.audio.compressor` | Compressor | nw.audio.dynamics.compressor | native | core | acompressor,alimiter |
| `ec.audio.delay` | Delay | nw.audio.delay.echo | composed | core | adelay,aecho,amix |
| `ec.audio.distortion` | Distortion | nw.audio.distortion | composed | core | asoftclip,acrusher,volume,amix |
| `ec.audio.flangechorus` | Flange & Chorus | nw.audio.modulation.chorus_flanger | native | core | chorus,flanger |
| `ec.audio.gate` | Gate | nw.audio.dynamics.gate | native | partial | agate |
| `ec.audio.highlowpass` | High-Low Pass | nw.audio.eq.high_low_pass | native | core | highpass,lowpass |
| `ec.audio.modulator` | Modulator | nw.audio.modulation | composed | core | tremolo,vibrato,aeval |
| `ec.audio.parametriceq` | Parametric EQ | nw.audio.eq.parametric_full | native | core | equalizer |
| `ec.audio.reverb` | Reverb | nw.audio.reverb.convolution | component | core | afir,lowpass,amix |
| `ec.audio.stereomixer` | Stereo Mixer | nw.audio.channel_matrix | native | core | pan |
| `ec.audio.tone` | Tone | nw.audio.generate.tone | composed | core | sine,aevalsrc,anoisesrc,amix,atrim |
| `ec.blur.bilateral` | Bilateral Blur | nw.video.blur.bilateral | native | core | bilateral |
| `ec.blur.cccross` | CC Cross Blur | nw.video.blur.cross | composed | core | split,gblur,blend |
| `ec.blur.ccradial` | CC Radial Blur | nw.video.blur.radial | composed | core | geq,blend |
| `ec.blur.ccradialfast` | CC Radial Fast Blur | nw.video.blur.zoom | composed | core | geq,blend |
| `ec.blur.ccvector` | CC Vector Blur | nw.video.blur.vector | composed | partial | gblur,convolution,geq,displace,blend |
| `ec.blur.cameralens` | Camera Lens Blur | nw.video.blur.aperture | composed | partial | color,geq,convolve,varblur,maskedmerge |
| `ec.blur.camerashakedeblur` | Camera-Shake Deblur | nw.video.restore.automatic_shake_deblur | unsupported | none | deconvolve,blurdetect |
| `ec.blur.channel` | Channel Blur | nw.video.blur.channels | composed | core | extractplanes,gblur,mergeplanes |
| `ec.blur.compound` | Compound Blur | nw.video.blur.radius_map | native | core | varblur,geq,maskedmerge |
| `ec.blur.directional` | Directional Blur | nw.video.blur.directional | composed | core | rotate,gblur,crop |
| `ec.blur.fastbox` | Fast Box Blur | nw.video.blur.box | native | core | boxblur,avgblur |
| `ec.blur.gaussian` | Gaussian Blur | nw.video.blur.gaussian | native | core | gblur |
| `ec.blur.radial` | Radial Blur | nw.video.blur.radial | composed | core | geq,blend |
| `ec.blur.sharpen` | Sharpen | nw.video.sharpen.standard | native | core | unsharp |
| `ec.blur.smart` | Smart Blur | nw.video.blur.smart | composed | core | smartblur,edgedetect,blend |
| `ec.blur.unsharp` | Unsharp Mask | nw.video.sharpen.thresholded | composed | core | unsharp,split,gblur,blend,geq |
| `ec.channel.arithmetic` | Arithmetic | nw.video.channels.arithmetic | composed | core | lutrgb,geq |
| `ec.channel.blend` | Blend | nw.video.composite.blend | composed | core | scale,pad,blend,overlay |
| `ec.channel.cccomposite` | CC Composite | nw.video.composite.original | composed | core | split,blend,overlay |
| `ec.channel.calculations` | Calculations | nw.video.channels.calculate | composed | core | extractplanes,geq,blend,alphamerge |
| `ec.channel.combiner` | Channel Combiner | nw.video.channels.convert | composed | core | colorspace,geq,mergeplanes |
| `ec.channel.compoundarithmetic` | Compound Arithmetic | nw.video.channels.combine_arithmetic | composed | core | blend,lut2,geq |
| `ec.channel.invert` | Invert | nw.video.color.invert_channels | composed | core | negate,geq,blend |
| `ec.channel.minimax` | Minimax | nw.video.morphology.min_max | native | core | erosion,dilation,morpho |
| `ec.channel.removecolormatting` | Remove Color Matting | nw.video.alpha.remove_matte_color | composed | core | geq,unpremultiply,premultiply |
| `ec.channel.setchannels` | Set Channels | nw.video.channels.set_from_layers | composed | core | extractplanes,geq,mergeplanes |
| `ec.channel.setmatte` | Set Matte | nw.video.mask.track_matte | composed | core | alphaextract,geq,alphamerge,blend |
| `ec.channel.shiftchannels` | Shift Channels | nw.video.channels.swizzle | composed | core | shuffleplanes,colorchannelmixer,geq |
| `ec.channel.solidcomposite` | Solid Composite | nw.video.composite.solid | composed | core | color,blend,overlay |
| `ec.color.autocolor` | Auto Color | nw.video.color.auto_balance | composed | core | normalize,colorcorrect,grayworld,blend |
| `ec.color.autocontrast` | Auto Contrast | nw.video.color.auto_contrast | native | core | normalize |
| `ec.color.autolevels` | Auto Levels | nw.video.color.auto_levels | native | core | normalize |
| `ec.color.blackwhite` | Black & White | nw.video.color.monochrome | composed | core | geq,colorchannelmixer,blend |
| `ec.color.brightnesscontrast` | Brightness & Contrast | nw.video.color.brightness_contrast | native | core | eq |
| `ec.color.broadcast` | Broadcast Colors | nw.video.color.signal_limiter | composed | partial | colorspace,limiter,geq |
| `ec.color.cccolorneutralizer` | CC Color Neutralizer | nw.video.color.neutralize | composed | core | colorbalance,curves,eq,blend |
| `ec.color.cccoloroffset` | CC Color Offset | nw.video.color.channel_phase | composed | core | lutrgb,geq |
| `ec.color.cckernel` | CC Kernel | nw.video.filter.convolution | native | core | convolution,geq |
| `ec.color.cctoner` | CC Toner | nw.video.color.multitone | composed | core | geq,lut1d,blend |
| `ec.color.changecolor` | Change Color | nw.video.color.change_selected | composed | core | hsvkey,geq,hue,huesaturation,maskedmerge |
| `ec.color.changetocolor` | Change to Color | nw.video.color.replace_selected | composed | core | geq,hsvkey,huesaturation,maskedmerge |
| `ec.color.channelmixer` | Channel Mixer | nw.video.color.channel_matrix | composed | core | colorchannelmixer,geq |
| `ec.color.colorbalance` | Color Balance | nw.video.color.tonal_balance | native | core | colorbalance |
| `ec.color.colorbalancehls` | Color Balance (HLS) | nw.video.color.hsl_adjust | composed | core | hue,huesaturation,geq |
| `ec.color.colorlink` | Color Link | nw.video.color.link_sample | composed | partial | crop,scale,geq,blend |
| `ec.color.colorstabilizer` | Color Stabilizer | nw.video.color.stabilize | composed | partial | deflicker,normalize,signalstats,eq,sendcmd |
| `ec.color.colorama` | Colorama | nw.video.color.palette_cycle | composed | core | geq,lut1d,lut3d,maskedmerge |
| `ec.color.curves` | Curves | nw.video.color.curves | native | core | curves,lut1d,geq |
| `ec.color.equalize` | Equalize | nw.video.color.histogram_equalize | native | core | histeq,blend |
| `ec.color.exposure` | Exposure | nw.video.color.exposure | composed | core | exposure,lutrgb,geq |
| `ec.color.gammapedestalgain` | Gamma/Pedestal/Gain | nw.video.color.lift_gamma_gain | composed | core | lutrgb,geq |
| `ec.color.huesaturation` | Hue/Saturation | nw.video.color.hue_saturation | native | core | hue,huesaturation |
| `ec.color.leavecolor` | Leave Color | nw.video.color.rgb_isolation; conditional: nw.video.color.hue_isolation | native | core | colorhold,hsvhold,blend |
| `ec.color.levels` | Levels | nw.video.color.levels | composed | core | colorlevels,lutrgb,geq |
| `ec.color.levelsic` | Levels (Individual Controls) | nw.video.color.levels | composed | core | colorlevels,lutrgb,geq |
| `ec.color.lumetri` | Lumetri Color | nw.video.color.grading_stack | composed | core | lut3d,colortemperature,colorbalance,curves,huesaturation,vibrance,vignette,unsharp,maskedmerge |
| `ec.color.ociocdl` | OCIO CDL Transform | nw.video.color.asc_cdl | composed | core | geq,lut3d |
| `ec.color.ociocolorspace` | OCIO Color Space Transform | nw.video.color.ocio_space | component | core | ocio |
| `ec.color.ociodisplay` | OCIO Display Transform | nw.video.color.ocio_display | component | core | ocio |
| `ec.color.ociofile` | OCIO File Transform | nw.video.color.ocio_file | component | partial | ocio,lut1d,lut3d |
| `ec.color.ociolook` | OCIO Look Transform | nw.video.color.ocio_look | component | partial | ocio |
| `ec.color.psarbitrarymap` | PS Arbitrary Map | nw.video.color.channel_map | composed | core | lut1d,lutrgb,geq |
| `ec.color.photofilter` | Photo Filter | nw.video.color.photo_filter | composed | core | colorchannelmixer,geq,blend |
| `ec.color.selectivecolor` | Selective Color | nw.video.color.selective_cmyk | native | core | selectivecolor |
| `ec.color.shadowhighlight` | Shadow/Highlight | nw.video.color.shadow_highlight | composed | core | gblur,geq,maskedmerge,curves,normalize |
| `ec.color.tint` | Tint | nw.video.color.two_color_tint | composed | core | geq,blend |
| `ec.color.tritone` | Tritone | nw.video.color.multitone | composed | core | geq,lut1d,blend |
| `ec.color.vibrance` | Vibrance | nw.video.color.vibrance | native | core | vibrance,hue |
| `ec.color.videolimiter` | Video Limiter | nw.video.color.signal_limiter | composed | core | limiter,geq,maskedmerge |
| `ec.distort.bezierwarp` | Bezier Warp | nw.video.warp.bezier | composed | partial | perspective,geq,remap,overlay |
| `ec.distort.bulge` | Bulge | nw.video.warp.bulge | composed | core | geq |
| `ec.distort.ccbendit` | CC Bend It | nw.video.warp.bend_segment | composed | core | geq |
| `ec.distort.ccbender` | CC Bender | nw.video.warp.bend | composed | core | geq |
| `ec.distort.ccblobbylize` | CC Blobbylize | nw.video.relief.blob | composed | partial | gblur,geq,displace,blend,alphamerge |
| `ec.distort.ccflomotion` | CC Flo Motion | nw.video.warp.two_point_flow | composed | core | geq |
| `ec.distort.ccgriddler` | CC Griddler | nw.video.warp.grid_tiles | composed | core | geq |
| `ec.distort.cclens` | CC Lens | nw.video.warp.lens | composed | core | lenscorrection,geq |
| `ec.distort.ccpageturn` | CC Page Turn | nw.transition.page.turn | composed | partial | geq,blend,maskedmerge |
| `ec.distort.ccpowerpin` | CC Power Pin | nw.video.geometry.corner_pin | composed | core | perspective,pad,crop |
| `ec.distort.ccripplepulse` | CC Ripple Pulse | nw.video.warp.ripple_pulse | composed | core | geq |
| `ec.distort.ccslant` | CC Slant | nw.video.warp.slant | composed | core | shear,geq |
| `ec.distort.ccsmear` | CC Smear | nw.video.warp.smear_segment | composed | core | geq |
| `ec.distort.ccsplit` | CC Split | nw.video.warp.split | composed | core | geq |
| `ec.distort.ccsplit2` | CC Split 2 | nw.video.warp.split_independent | composed | core | geq |
| `ec.distort.cctiler` | CC Tiler | nw.video.tile.repeat | composed | core | geq,blend |
| `ec.distort.cornerpin` | Corner Pin | nw.video.geometry.corner_pin | native | core | perspective |
| `ec.distort.upscale` | Detail-preserving Upscale | nw.video.resize.detail | composed | core | scale,zscale,nlmeans,unsharp |
| `ec.distort.displacementmap` | Displacement Map | nw.video.warp.displacement_map | composed | core | geq,displace,remap,pad |
| `ec.distort.liquify` | Liquify | nw.video.warp.liquify | composed | partial | geq,remap |
| `ec.distort.magnify` | Magnify | nw.video.geometry.loupe | composed | core | geq,maskedmerge |
| `ec.distort.meshwarp` | Mesh Warp | nw.video.warp.mesh | composed | partial | perspective,geq,remap,overlay |
| `ec.distort.mirror` | Mirror | nw.video.geometry.mirror_line | composed | core | hflip,vflip,geq |
| `ec.distort.offset` | Offset | nw.video.geometry.wrap_offset | composed | core | scroll,geq,blend |
| `ec.distort.opticscompensation` | Optics Compensation | nw.video.warp.optics_compensation | composed | core | lenscorrection,v360,geq |
| `ec.distort.polar` | Polar Coordinates | nw.video.warp.polar_coordinates | composed | core | geq |
| `ec.distort.puppet` | Puppet | nw.video.deform.puppet_solver | unsupported | none | perspective,geq,remap |
| `ec.distort.reshape` | Reshape | nw.video.warp.reshape_paths | composed | partial | geq,remap,maskedmerge |
| `ec.distort.ripple` | Ripple | nw.video.warp.ripple | composed | core | geq |
| `ec.distort.rollingshutterrepair` | Rolling Shutter Repair | nw.video.restore.rolling_shutter_auto | unsupported | none | geq,remap |
| `ec.distort.smear` | Smear | nw.video.warp.smear_path | composed | partial | geq,remap,maskedmerge |
| `ec.distort.spherize` | Spherize | nw.video.geometry.spherize | composed | core | geq |
| `ec.distort.transform` | Transform | nw.video.geometry.transform | composed | core | scale,rotate,shear,crop,pad,overlay,geq |
| `ec.distort.turbulentdisplace` | Turbulent Displace | nw.video.geometry.turbulence | composed | core | perlin,geq,displace,mergeplanes,vstack,crop |
| `ec.distort.twirl` | Twirl | nw.video.geometry.twirl | composed | core | geq |
| `ec.distort.twirllegacy` | Twirl (Legacy) | nw.video.geometry.twirl | composed | core | geq |
| `ec.distort.warp` | Warp | nw.video.warp.presets | composed | core | geq,perspective,shear |
| `ec.distort.warpstabilizer` | Warp Stabilizer | nw.video.stabilize | component | partial | vidstabdetect,vidstabtransform,deshake |
| `ec.distort.wavewarp` | Wave Warp | nw.video.geometry.wave | composed | core | geq |
| `ec.control.point3d` | 3D Point Control | nw.project.parameter.point3 | application | control | null |
| `ec.control.angle` | Angle Control | nw.project.parameter.angle | application | control | null |
| `ec.control.checkbox` | Checkbox Control | nw.project.parameter.boolean | application | control | null |
| `ec.control.color` | Color Control | nw.project.parameter.color | application | control | null |
| `ec.control.dropdown` | Dropdown Menu Control | nw.project.parameter.enum | application | control | null |
| `ec.control.layer` | Layer Control | nw.project.parameter.asset_ref | application | control | null |
| `ec.control.point` | Point Control | nw.project.parameter.point2 | application | control | null |
| `ec.control.slider` | Slider Control | nw.project.parameter.scalar | application | control | null |
| `ec.control.stereo3d` | Stereo 3D Controls | nw.project.parameter.stereo_rig | application | control | null |
| `ec.generate.fourcolor` | 4-Color Gradient | nw.video.generate.four_color_field | composed | core | color,geq,blend |
| `ec.generate.advancedlightning` | Advanced Lightning | nw.video.generate.lightning | component | partial | drawvg,gblur,blend |
| `ec.generate.audiospectrum` | Audio Spectrum | nw.video.visualize.audio_spectrum | native | core | showfreqs,showspectrum,overlay |
| `ec.generate.audiowaveform` | Audio Waveform | nw.video.visualize.audio_waveform | native | core | showwaves,showwavespic,overlay |
| `ec.generate.beam` | Beam | nw.video.generate.beam | composed | core | color,geq,gblur,overlay |
| `ec.generate.ccgluegun` | CC Glue Gun | nw.video.generate.glue_stroke | component | partial | drawvg,gblur,displace,blend |
| `ec.generate.cclightburst` | CC Light Burst 2.5 | nw.video.light.radial_rays | composed | core | geq,blend |
| `ec.generate.cclightrays` | CC Light Rays | nw.video.light.radial_rays | composed | core | geq,blend |
| `ec.generate.cclightsweep` | CC Light Sweep | nw.video.light.sweep | composed | core | geq,alphaextract,gblur,blend |
| `ec.generate.ccthreads` | CC Threads | nw.video.texture.weave | composed | core | geq,blend |
| `ec.generate.cellpattern` | Cell Pattern | nw.video.generate.cellular_noise | composed | core | color,geq |
| `ec.generate.checkerboard` | Checkerboard | nw.video.generate.checkerboard | composed | core | color,geq,blend |
| `ec.generate.circle` | Circle | nw.video.generate.circle | composed | core | color,geq,overlay |
| `ec.generate.ellipse` | Ellipse | nw.video.generate.ellipse_ring | composed | core | color,geq,overlay |
| `ec.generate.eyedropperfill` | Eyedropper Fill | nw.video.generate.sample_fill | composed | core | crop,scale,geq,blend |
| `ec.generate.fill` | Fill | nw.video.generate.fill | composed | core | color,geq,gblur,maskedmerge |
| `ec.generate.fractal` | Fractal | nw.video.generate.fractal_mandelbrot | native | partial | mandelbrot |
| `ec.generate.gradientramp` | Gradient Ramp | nw.video.generate.gradient | native | core | gradients,color,geq,blend |
| `ec.generate.grid` | Grid | nw.video.generate.grid | composed | core | drawgrid,geq,blend |
| `ec.generate.lensflare` | Lens Flare | nw.video.light.lens_flare | composed | core | color,geq,gblur,blend |
| `ec.generate.paintbucket` | Paint Bucket | nw.video.fill.connected | composed | core | geq,floodfill,erosion,dilation,gblur,maskedmerge |
| `ec.generate.radiowaves` | Radio Waves | nw.video.generate.radio_waves | component | partial | drawvg,geq,overlay |
| `ec.generate.scribble` | Scribble | nw.video.generate.scribble | component | core | drawvg,overlay |
| `ec.generate.stroke` | Stroke | nw.video.paint.stroke | component | core | drawvg,gblur,overlay |
| `ec.generate.vegas` | Vegas | nw.video.generate.vegas_stroke | component | partial | drawvg,geq,overlay |
| `ec.generate.writeon` | Write-on | nw.video.paint.write_on | component | core | drawvg,gblur,overlay |
| `ec.vr.blur` | VR Blur | nw.video.vr.blur | composed | core | v360,gblur,crop,hstack,vstack |
| `ec.vr.chromaticaberrations` | VR Chromatic Aberrations | nw.video.vr.chromatic_aberration | composed | core | extractplanes,v360,geq,mergeplanes |
| `ec.vr.colorgradients` | VR Color Gradients | nw.video.vr.color_field | composed | core | geq,blend |
| `ec.vr.converter` | VR Converter | nw.video.vr.projection_convert | native | core | v360 |
| `ec.vr.denoise` | VR De-Noise | nw.video.vr.denoise | composed | core | v360,nlmeans,hqdn3d,crop,hstack,vstack |
| `ec.vr.digitalglitch` | VR Digital Glitch | nw.video.vr.glitch | composed | core | geq,rgbashift,blend |
| `ec.vr.fractalnoise` | VR Fractal Noise | nw.video.vr.fractal_noise | composed | partial | color,geq,blend |
| `ec.vr.glow` | VR Glow | nw.video.vr.glow | composed | core | v360,geq,gblur,blend |
| `ec.vr.planetosphere` | VR Plane to Sphere | nw.video.vr.tiny_planet | native | core | v360 |
| `ec.vr.rotatesphere` | VR Rotate Sphere | nw.video.vr.rotate | native | core | v360 |
| `ec.vr.sharpen` | VR Sharpen | nw.video.vr.sharpen | composed | core | v360,unsharp,crop,hstack,vstack |
| `ec.vr.spheretoplane` | VR Sphere to Plane | nw.video.vr.view_extract | native | core | v360 |
| `ec.key.advancedspill` | Advanced Spill Suppressor | nw.video.key.spill_advanced | composed | core | despill,geq,huesaturation,maskedmerge |
| `ec.key.ccsimplewireremoval` | CC Simple Wire Removal | nw.video.restore.wire_remove | composed | core | geq,maskedmerge |
| `ec.key.colordifference` | Color Difference Key | nw.video.key.color_difference | composed | core | geq,colorlevels,alphamerge |
| `ec.key.colorrange` | Color Range | nw.video.key.color_range | composed | core | geq,hsvkey,alphamerge |
| `ec.key.differencematte` | Difference Matte | nw.video.key.difference_matte | composed | core | gblur,blend,geq,alphamerge |
| `ec.key.extract` | Extract | nw.video.key.extract_range | composed | core | lumakey,geq,alphamerge |
| `ec.key.innerouter` | Inner/Outer Key | nw.video.matte.inner_outer | composed | partial | gblur,guided,geq,maskedmerge,alphamerge |
| `ec.key.keycleaner` | Key Cleaner | nw.video.matte.key_cleanup | composed | core | alphaextract,guided,median,gblur,geq,alphamerge |
| `ec.keying.keylight` | Key Light | nw.video.key.advanced_chroma | composed | core | chromakey,despill,geq,erosion,dilation,gblur,maskedmerge,colorbalance |
| `ec.key.linearcolor` | Linear Color Key | nw.video.key.linear_color | composed | core | colorkey,hsvkey,geq,alphamerge |
| `ec.key.screen` | Screen Key | nw.video.key.advanced_chroma | composed | core | chromakey,despill,erosion,dilation,gblur,geq |
| `ec.key.unmult` | Unmult | nw.video.key.flat_background | composed | core | geq,unpremultiply,alphamerge |
| `ec.matte.mattechoker` | Matte Choker | nw.video.matte.choker | composed | core | alphaextract,gblur,geq,erosion,dilation,alphamerge |
| `ec.obsolete.mochashape` | Mocha shape | nw.video.matte.vector_shape | component | core | drawvg,gblur,geq,alphamerge |
| `ec.matte.objectmatte` | Object Matte | nw.video.matte.object_auto | unsupported | none | alphamerge,maskedmerge |
| `ec.matte.refinehard` | Refine Hard Matte | nw.video.matte.refine_hard | composed | core | guided,median,erosion,dilation,gblur,geq,alphamerge |
| `ec.matte.refinesoft` | Refine Soft Matte | nw.video.matte.refine_soft | composed | core | guided,gblur,erosion,dilation,geq,alphamerge |
| `ec.matte.rotobrush` | Roto Brush & Refine Edge | nw.video.matte.rotobrush_auto | unsupported | none | drawvg,alphamerge,guided |
| `ec.matte.simplechoker` | Simple Choker | nw.video.matte.choke | native | core | erosion,dilation,morpho,alphaextract,alphamerge |
| `ec.noise.addgrain` | Add Grain | nw.video.noise.grain | composed | core | noise,scale,gblur,geq,blend |
| `ec.noise.curlnoise` | Curl Noise | nw.video.warp.curl_noise | composed | partial | perlin,convolution,geq,displace |
| `ec.noise.dustscratches` | Dust & Scratches | nw.video.restore.dust_scratches | composed | core | median,blend,geq,maskedmerge |
| `ec.noise.fractal` | Fractal Noise | nw.video.generate.fractal_noise | composed | core | perlin,geq,rotate,scale,blend |
| `ec.noise.matchgrain` | Match Grain | nw.video.noise.grain_match | composed | partial | gblur,blend,geq,scale,signalstats,noise |
| `ec.noise.median` | Median | nw.video.denoise.median | native | core | median |
| `ec.noise.medianlegacy` | Median (Legacy) | nw.video.denoise.median | native | core | median |
| `ec.noise.noise` | Noise | nw.video.noise.grain | native | core | noise,scale,format,geq,mergeplanes,blend,vstack,crop |
| `ec.noise.noisealpha` | Noise Alpha | nw.video.noise.alpha | composed | core | alphaextract,noise,geq,alphamerge |
| `ec.noise.noisehls` | Noise HLS | nw.video.noise.hsl | composed | core | noise,geq,mergeplanes |
| `ec.noise.noisehlsauto` | Noise HLS Auto | nw.video.noise.hsl | composed | core | noise,geq,mergeplanes |
| `ec.noise.removegrain` | Remove Grain | nw.video.denoise.grain | composed | core | hqdn3d,nlmeans,atadenoise,unsharp |
| `ec.noise.turbulent` | Turbulent Noise | nw.video.generate.turbulent_noise | composed | core | perlin,geq,displace,blend |
| `ec.obsolete.basic3d` | Basic 3D | nw.video.geometry.planar_3d | composed | partial | perspective,geq |
| `ec.obsolete.basictext` | Basic Text | nw.video.text.burnin | component | core | drawtext,subtitles |
| `ec.key.colorkey` | Color Key | nw.video.key.rgb | composed | core | colorkey,erosion,dilation,gblur,alphamerge |
| `ec.obsolete.gaussianlegacy` | Gaussian Blur (Legacy) | nw.video.blur.gaussian | native | core | gblur |
| `ec.obsolete.lightning` | Lightning | nw.video.generate.lightning | component | partial | drawvg,gblur,blend |
| `ec.key.luma` | Luma Key | nw.video.key.luma | composed | core | lumakey,erosion,dilation,gblur,alphamerge |
| `ec.obsolete.pathtext` | Path Text | nw.video.text.path | component | core | drawvg,overlay |
| `ec.blur.reduceflicker` | Reduce Interlace Flicker | nw.video.blur.vertical_soften | native | core | convolution |
| `ec.key.spill` | Spill Suppressor | nw.video.key.spill_suppress | native | core | despill |
| `ec.paint.paint` | Paint | nw.video.paint.vector | component | partial | drawvg,gblur,overlay,maskedmerge |
| `ec.perspective.cameratracker` | 3D Camera Tracker | nw.video.analysis.camera_solve | unsupported | none | drawvg,lenscorrection |
| `ec.perspective.3dglasses` | 3D Glasses | nw.video.stereo.compose | composed | core | stereo3d,pad,crop,hstack,colorchannelmixer |
| `ec.perspective.bevelalpha` | Bevel Alpha | nw.video.relief.alpha_bevel | composed | core | alphaextract,gblur,convolution,geq,blend |
| `ec.perspective.beveledges` | Bevel Edges | nw.video.relief.bevel_edges | composed | core | geq,convolution,blend |
| `ec.perspective.cccylinder` | CC Cylinder | nw.video.warp.cylinder | composed | partial | geq |
| `ec.perspective.ccenvironment` | CC Environment | nw.video.relief.environment | composed | partial | gblur,convolution,geq,remap,blend |
| `ec.perspective.ccsphere` | CC Sphere | nw.video.warp.sphere | composed | partial | v360,geq,blend |
| `ec.perspective.ccspotlight` | CC Spotlight | nw.video.light.surface_lighting | composed | core | geq,blend |
| `ec.perspective.dropshadow` | Drop Shadow | nw.video.shadow.drop | composed | core | alphaextract,gblur,color,alphamerge,pad,overlay |
| `ec.perspective.radialshadow` | Radial Shadow | nw.video.shadow.radial | composed | core | alphaextract,geq,gblur,blend,overlay |
| `ec.sim.ccballaction` | CC Ball Action | nw.video.simulation.ball_action | unsupported | none | pixelize,geq |
| `ec.sim.ccbubbles` | CC Bubbles | nw.video.generate.bubbles | composed | partial | color,geq,overlay |
| `ec.sim.ccdrizzle` | CC Drizzle | nw.video.generate.drizzle_ripples | composed | partial | geq,displace,blend |
| `ec.sim.cchair` | CC Hair | nw.video.simulation.surface_hair | unsupported | none | drawvg |
| `ec.sim.ccmrmercury` | CC Mr. Mercury | nw.video.simulation.liquid_blobs | unsupported | none | color,geq,displace |
| `ec.sim.ccparticlesystems2` | CC Particle Systems II | nw.video.simulation.particles_2d | unsupported | none | drawvg,overlay |
| `ec.sim.ccparticleworld` | CC Particle World | nw.video.simulation.particles_3d | unsupported | none | drawvg,perspective,overlay |
| `ec.sim.ccpixelpolly` | CC Pixel Polly | nw.video.simulation.pixel_fragments | unsupported | none | crop,perspective,overlay |
| `ec.sim.ccrainfall` | CC Rainfall | nw.video.generate.rain | composed | partial | color,geq,blend |
| `ec.sim.ccscatterize` | CC Scatterize | nw.video.simulation.pixel_scatter | unsupported | none | shufflepixels,noise,displace |
| `ec.sim.ccsnowfall` | CC Snowfall | nw.video.generate.snow | composed | partial | color,geq,blend |
| `ec.sim.ccstarburst` | CC Star Burst | nw.video.generate.starburst | composed | partial | color,noise,geq,zoompan,blend |
| `ec.sim.carddance` | Card Dance | nw.video.simulation.card_dance | unsupported | none | crop,perspective,overlay |
| `ec.sim.caustics` | Caustics | nw.video.warp.water_caustic | composed | partial | gblur,convolution,geq,displace,blend |
| `ec.sim.foam` | Foam | nw.video.simulation.foam | unsupported | none | geq,overlay |
| `ec.sim.particleplayground` | Particle Playground | nw.video.simulation.particle_playground | unsupported | none | drawvg,overlay |
| `ec.sim.shatter` | Shatter | nw.video.simulation.shatter | unsupported | none | crop,perspective,overlay |
| `ec.sim.waveworld` | Wave World | nw.video.simulation.wave_solver | unsupported | none | perlin,geq |
| `ec.stylize.brushstrokes` | Brush Strokes | nw.video.stylize.brush_strokes | composed | core | rotate,scale,geq,overlay |
| `ec.stylize.ccblockload` | CC Block Load | nw.video.stylize.block_load | composed | core | pixelize,scale,geq,blend |
| `ec.stylize.ccburnfilm` | CC Burn Film | nw.video.transition.film_burn | composed | core | perlin,geq,maskedmerge,blend |
| `ec.stylize.ccglass` | CC Glass | nw.video.relief.glass | composed | core | gblur,convolution,geq,displace,blend |
| `ec.stylize.cchextile` | CC HexTile | nw.video.tile.hex | composed | core | geq |
| `ec.stylize.cckaleida` | CC Kaleida | nw.video.tile.kaleidoscope | composed | core | geq |
| `ec.stylize.ccmrsmoothie` | CC Mr. Smoothie | nw.video.color.sampled_palette_flow | composed | core | gblur,geq,mergeplanes |
| `ec.stylize.ccplastic` | CC Plastic | nw.video.relief.plastic | composed | core | gblur,convolution,geq,blend |
| `ec.stylize.ccrepetile` | CC RepeTile | nw.video.tile.expand_repeat | composed | core | pad,geq |
| `ec.stylize.ccthreshold` | CC Threshold | nw.video.color.threshold | composed | core | geq,lutrgb,blend |
| `ec.stylize.ccthresholdrgb` | CC Threshold RGB | nw.video.color.threshold_rgb | composed | core | lutrgb,blend |
| `ec.stylize.ccvignette` | CC Vignette | nw.video.light.vignette | native | core | vignette,geq,blend |
| `ec.stylize.cartoon` | Cartoon | nw.video.stylize.cartoon | composed | core | bilateral,elbg,lutrgb,edgedetect,gblur,blend |
| `ec.stylize.coloremboss` | Color Emboss | nw.video.relief.color_emboss | composed | core | convolution,geq,blend |
| `ec.stylize.emboss` | Emboss | nw.video.relief.emboss | composed | core | convolution,geq,blend |
| `ec.stylize.findedges` | Find Edges | nw.video.edges | native | core | edgedetect,sobel,geq,negate,blend |
| `ec.stylize.glow` | Glow | nw.video.light.glow | composed | core | geq,gblur,colorchannelmixer,blend |
| `ec.stylize.mosaic` | Mosaic | nw.video.pixelate | native | core | pixelize,scale |
| `ec.stylize.motiontile` | Motion Tile | nw.video.tile.motion | composed | core | geq,pad |
| `ec.stylize.posterize` | Posterize | nw.video.color.posterize | composed | core | lutrgb,geq |
| `ec.stylize.roughenedges` | Roughen Edges | nw.video.mask.roughen | composed | core | alphaextract,perlin,gblur,geq,alphamerge,blend |
| `ec.stylize.scatter` | Scatter | nw.video.stylize.scatter | composed | core | noise,geq,displace,shufflepixels |
| `ec.stylize.strobe` | Strobe Light | nw.video.light.strobe | composed | core | geq,blend |
| `ec.stylize.texturize` | Texturize | nw.video.relief.texturize | composed | core | scale,geq,convolution,blend |
| `ec.stylize.threshold` | Threshold | nw.video.color.threshold | composed | core | geq,lutrgb |
| `ec.text.numbers` | Numbers | nw.video.text.number_overlay | component | core | drawtext,subtitles |
| `ec.text.timecode` | Timecode | nw.video.text.timecode | component | core | drawtext,subtitles |
| `ec.time.ccforcemotionblur` | CC Force Motion Blur | nw.video.time.motion_blur | composed | core | minterpolate,tmix,fps,setpts |
| `ec.time.ccwidetime` | CC Wide Time | nw.video.time.wide_average | composed | core | tmix,tpad,setpts |
| `ec.time.echo` | Echo | nw.video.time.echo | composed | core | split,trim,setpts,tpad,blend |
| `ec.time.pixelmotionblur` | Pixel Motion Blur | nw.video.time.optical_motion_blur | composed | core | minterpolate,tmix,fps |
| `ec.time.posterizetime` | Posterize Time | nw.video.time.hold_rate | composed | core | fps,setpts,tpad |
| `ec.time.timedifference` | Time Difference | nw.video.time.difference | composed | core | trim,setpts,blend,geq,alphamerge |
| `ec.time.timedisplacement` | Time Displacement | nw.video.time.displacement_map | composed | partial | trim,setpts,geq,maskedmerge |
| `ec.time.timewarp` | Timewarp | nw.video.time.retime | composed | core | setpts,fps,minterpolate,trim,concat,tmix |
| `ec.transition.blockdissolve` | Block Dissolve | nw.video.transition.block_dissolve | composed | core | xfade,geq,maskedmerge |
| `ec.transition.ccglasswipe` | CC Glass Wipe | nw.video.transition.glass_wipe | composed | core | gblur,geq,displace,maskedmerge |
| `ec.transition.ccgridwipe` | CC Grid Wipe | nw.video.transition.grid_wipe | composed | core | xfade,geq,maskedmerge |
| `ec.transition.ccimagewipe` | CC Image Wipe | nw.video.transition.image_wipe | composed | core | gblur,geq,maskedmerge |
| `ec.transition.ccjaws` | CC Jaws | nw.video.transition.jaws | composed | core | geq,maskedmerge |
| `ec.transition.cclightwipe` | CC Light Wipe | nw.video.transition.light_wipe | composed | core | geq,gblur,maskedmerge,blend |
| `ec.transition.cclinesweep` | CC Line Sweep | nw.video.transition.line_sweep | composed | core | geq,maskedmerge |
| `ec.transition.ccradialscalewipe` | CC Radial ScaleWipe | nw.video.transition.radial_scale | composed | core | geq,maskedmerge |
| `ec.transition.ccscalewipe` | CC Scale Wipe | nw.video.transition.scale_wipe | composed | core | geq,maskedmerge,xfade |
| `ec.transition.cctwister` | CC Twister | nw.video.transition.twist | composed | core | geq,maskedmerge |
| `ec.transition.ccwarpomatic` | CC WarpoMatic | nw.video.transition.reactive_warp | composed | core | gblur,convolution,geq,displace,maskedmerge |
| `ec.transition.cardwipe` | Card Wipe | nw.video.transition.card_flip_grid | composed | partial | crop,perspective,overlay,geq |
| `ec.transition.gradientwipe` | Gradient Wipe | nw.transition.wipe.gradient | composed | core | geq,maskedmerge,scale,pad |
| `ec.transition.iriswipe` | Iris Wipe | nw.transition.wipe.iris | composed | core | xfade,geq,maskedmerge |
| `ec.transition.linearwipe` | Linear Wipe | nw.transition.wipe.linear | composed | core | xfade,geq,maskedmerge |
| `ec.transition.radialwipe` | Radial Wipe | nw.transition.wipe.clock | composed | core | xfade,geq,maskedmerge |
| `ec.transition.venetian` | Venetian Blinds | nw.transition.wipe.blinds | composed | core | geq,maskedmerge,xfade |
| `ec.utility.applylut` | Apply Color LUT | nw.video.color.lut_apply | native | core | lut1d,lut3d |
| `ec.utility.ccoverbrights` | CC Overbrights | nw.video.inspect.overbrights | composed | core | geq |
| `ec.utility.cineon` | Cineon Converter | nw.video.color.cineon | composed | core | geq,lut1d |
| `ec.utility.colorprofileconverter` | Color Profile Converter | nw.video.color.profile_convert | component | core | colorspace,zscale,scale,ocio |
| `ec.utility.facemeasurements` | Face Measurements | nw.project.parameter.face_measurements | application | control | null |
| `ec.utility.facetrackpoints` | Face Track Points | nw.project.parameter.face_landmarks | application | control | null |
| `ec.utility.growbounds` | Grow Bounds | nw.video.bounds.expand | native | core | pad,crop |
| `ec.utility.hdrcompander` | HDR Compander | nw.video.color.hdr_compand | composed | core | geq |
| `ec.utility.hdrcompression` | HDR Highlight Compression | nw.video.color.highlight_compress | composed | core | geq,tonemap |

#### Explicit boundaries of the 41 partial EffectCraft rows

These are required truthful feature limits, not an invitation to leave the entire parent workflow unfinished. The full combined CSV retains route, input/component and resource details for each row.

| Source feature | Admitted function and precise limitation | Input/component contract |
|---|---|---|
| 3D Channel Extract (`ec.3d.channelextract`) | Visualize/select auxiliary depth, numeric ID, normal or UV data. Arbitrary named EXR channel discovery is not established by these filters; require explicit input-plane mapping. | Synchronized auxiliary plane assets with declared scale, encoding and frame correspondence. |
| EXtractoR (`ec.3d.extractor`) | User-selectable channel remapping and level adjustment. Unknown channel names are errors; arbitrary multichannel EXR enumeration is not claimed. | Explicit decoded channel-to-plane inventory. |
| Gate (`ec.audio.gate`) | Noise gate/expander. A separate exact hold-duration stage is not supplied by agate; do not expose an inert donor hold parameter. | Declare detector and gain units. |
| CC Vector Blur (`ec.blur.ccvector`) | Guide-directed blur with angle/length modes. Full-resolution adaptive large-tap flow is excluded; this is not optical-flow motion blur. | Explicit guide interpretation and map precision; source/guide registration. |
| Camera Lens Blur (`ec.blur.cameralens`) | Aperture-shaped blur and highlight-aware compositing. Exact diffraction fringe and physically varying bokeh are unsupported; varblur mode is a declared simpler defocus variant. | Optional registered depth guide; aperture kernel identity. The varblur variant obeys its actual per-plane guide format/transfer and has an explicit zero-blur bypass, not an assumed min_r=0 identity. |
| Broadcast Colors (`ec.color.broadcast`) | Legal-range clamp/warning for an explicit delivery profile. Analog NTSC/PAL composite-signal certification is not established by simple component limits. | Delivery primaries/matrix/range/bit depth. |
| Color Link (`ec.color.colorlink`) | Link image color to another layer's sample. Mean and selected-point modes are admitted; automatic full-frame median color estimation is not included. | Explicit sample region/time/source; RGB versus alpha statistic. |
| Color Stabilizer (`ec.color.colorstabilizer`) | Stabilize brightness/levels relative to a declared reference. Exact donor three-point curve matching is not supplied automatically. | Source hash/time mapping; sample-region measurements from FFmpeg. |
| OCIO File Transform (`ec.color.ociofile`) | File-based color transform. Arbitrary CCC entry selectors or inverse interpolation not exposed by the pinned wrapper need an explicitly prepared transform file. | Qualified libopencolorio or supported native LUT parser; asset hash/dependency closure. |
| OCIO Look Transform (`ec.color.ociolook`) | Look processing is possible through the config, but the pinned ocio filter has no direct looks option; do not invent one. | Prepared validated config encodes requested look and direction; libopencolorio component. |
| Bezier Warp (`ec.distort.bezierwarp`) | Curved boundary warp with editable vertices/tangents. Dense adaptive tessellation and overlapping/self-intersecting patch precedence are outside the initial operation. | Explicit path geometry; transparent edge and sampling policy. |
| CC Blobbylize (`ec.distort.ccblobbylize`) | Blobby relief/keying from a guide. Host scene lights and full material response are not supported; provide a documented single-light 2D treatment. | Registered guide or source-derived channel; explicit matte. |
| CC Page Turn (`ec.distort.ccpageturn`) | Stylized page turn with fold position/direction/radius. Complex curled geometry/occlusion is not a general 3D paper simulation. | Optional backside layer; finite radius and explicit endpoints. |
| Liquify (`ec.distort.liquify`) | Small editable liquify distortions. Freeze-area diagnostics can use masks; unlimited dense brush history and reconstruction solver are unsupported. | Typed vector brush controls and optional explicit freeze mask. |
| Mesh Warp (`ec.distort.meshwarp`) | Editable coarse mesh deformation. Dense meshes, self-overlap ordering and arbitrary subdivision are not admitted. | Explicit mesh vertices with validated topology. |
| Reshape (`ec.distort.reshape`) | Morph/reshape between user-authored paths. Automatic correspondence and dense elastic solving are unsupported. | Typed vector paths and explicit correspondence. |
| Smear (`ec.distort.smear`) | Path-guided smear with strength and smooth falloff. Unlimited dense elastic deformation is not part of this initial operation. | Explicit source/boundary vector paths. |
| Warp Stabilizer (`ec.distort.warpstabilizer`) | Global camera stabilization with smoothing, crop/zoom and preview. Subspace warp, automatic rolling-shutter repair and synthetic border reconstruction remain unsupported. | libvidstab component for two-pass route; hashed analysis tied to input/preprocessing and source timing. |
| Advanced Lightning (`ec.generate.advancedlightning`) | Lightning-style visual generator with color/width/glow and limited forks. Alpha-obstacle collision routing and physical conductivity simulation are unsupported. | FFmpeg drawvg/Cairo build; typed vector geometry. Rasterize generated geometry/coverage on a separate negotiated Cairo surface with explicit premultiplied-alpha handling; convert/composite that asset into the working domain. Do not pass an HDR/float source frame through drawvg and silently quantize it. |
| CC Glue Gun (`ec.generate.ccgluegun`) | Glue-like stroke visual. Full donor bead accumulation, physical reflection and unlimited brush history are not admitted. | FFmpeg drawvg/Cairo; typed bounded stroke history. Rasterize generated geometry/coverage on a separate negotiated Cairo surface with explicit premultiplied-alpha handling; convert/composite that asset into the working domain. Do not pass an HDR/float source frame through drawvg and silently quantize it. |
| Fractal (`ec.generate.fractal`) | Mandelbrot fractal generation is admitted. Arbitrary Julia equations and donor-specific equation variants are unsupported unless a practical native FFmpeg path is added. | Finite duration/resolution and explicit palette/iteration settings. |
| Radio Waves (`ec.generate.radiowaves`) | Repeating wave-outline animation. Automatic image-contour tracing and reflecting wave physics are unsupported. | FFmpeg drawvg/Cairo or analytic shape masks; explicit vector path. Rasterize generated geometry/coverage on a separate negotiated Cairo surface with explicit premultiplied-alpha handling; convert/composite that asset into the working domain. Do not pass an HDR/float source frame through drawvg and silently quantize it. |
| Vegas (`ec.generate.vegas`) | Animated illuminated path segments. Automatic contour extraction from image pixels is unsupported. | FFmpeg drawvg/Cairo; explicit supplied path. Rasterize generated geometry/coverage on a separate negotiated Cairo surface with explicit premultiplied-alpha handling; convert/composite that asset into the working domain. Do not pass an HDR/float source frame through drawvg and silently quantize it. |
| VR Fractal Noise (`ec.vr.fractalnoise`) | Seam-continuous animated multiscale spherical texture. It is a documented lightweight texture variant, not the donor's full 3D value-noise/fractal-type engine. | Explicit sphere convention, seed and finite duration. |
| Inner/Outer Key (`ec.key.innerouter`) | Mask-assisted soft matting with editable foreground/background and cleanup masks. Automatic segmentation/path acquisition is not included. | Supplied vector/bitmap masks; aligned guide; guided second input is the guidance image with matching format/size. |
| Curl Noise (`ec.noise.curlnoise`) | Curl-like flow distortion and vector preview. Initial operation uses one to four finite advection stages, not a persistent fluid solver or donor 16-step identity. | Seeded guide and explicit coordinate/edge range. |
| Match Grain (`ec.noise.matchgrain`) | Match approximate grain amplitude/color to a reference with user-controlled grain size. Automatic exact film-stock spatial correlation/size recovery is unsupported. | Source/reference patches, recorded estimator and calibration; host consumes numerical FFmpeg statistics only. |
| Basic 3D (`ec.obsolete.basic3d`) | Perspective card treatment of a layer. General 3D scene lighting, geometry intersection and comp-camera rigs are unsupported. | Nondegenerate projected quad and explicit near-plane rejection. |
| Lightning (`ec.obsolete.lightning`) | Classic lightning visual shares the canonical generator. Unlimited recursive branches and physics/collision effects remain excluded. | FFmpeg drawvg/Cairo and typed vector controls. Rasterize generated geometry/coverage on a separate negotiated Cairo surface with explicit premultiplied-alpha handling; convert/composite that asset into the working domain. Do not pass an HDR/float source frame through drawvg and silently quantize it. |
| Paint (`ec.paint.paint`) | Paint/erase workflow for explicit strokes. Clone painting with arbitrary source times, dense per-channel brush simulation and unlimited histories are unsupported. | FFmpeg drawvg/Cairo; typed strokes and keyframes. Rasterize generated geometry/coverage on a separate negotiated Cairo surface with explicit premultiplied-alpha handling; convert/composite that asset into the working domain. Do not pass an HDR/float source frame through drawvg and silently quantize it. |
| CC Cylinder (`ec.perspective.cccylinder`) | Cylinder-style texture wrapping of a single layer. Arbitrary 3D camera/lighting/backface scene composition is unsupported. | Explicit radius/orientation, single-surface model and transparent exterior. |
| CC Environment (`ec.perspective.ccenvironment`) | Bump-map environment reflection as a 2D effect. Scene environment acquisition and general material/lighting are unsupported. | Registered height and environment assets; explicit projection/edge behavior. |
| CC Sphere (`ec.perspective.ccsphere`) | Sphere-looking wrap of one panoramic texture. No general 3D scene, arbitrary occlusion or physical material renderer. | Declared projection/rotation and transparent exterior. |
| CC Bubbles (`ec.sim.ccbubbles`) | Lightweight animated bubble overlay. Full refractive source sampling, depth sorting and material response are outside the template. | Explicit finite duration, cell density and seed. |
| CC Drizzle (`ec.sim.ccdrizzle`) | Drizzle-like ripple overlay/warp. It is a 2D procedural template without collision/physical water simulation. | Explicit ring lifetime/rate/seed and time origin. |
| CC Rainfall (`ec.sim.ccrainfall`) | Lightweight rain overlay. Donor refracting-drop appearance and continuous 3D depth behavior are not part of this 2D template. | Finite duration, seed and explicit layer count. |
| CC Snowfall (`ec.sim.ccsnowfall`) | Lightweight snow overlay with visual size/depth variation. Full donor per-flake projection and arbitrary distributions are outside the template. | Finite duration and explicit seed/layer count. |
| CC Star Burst (`ec.sim.ccstarburst`) | Flying-star/starburst graphic. Source-colored grid-pixel spheres and their 3D depth ordering are not claimed. | Finite output cadence and explicit seed/duration. |
| Caustics (`ec.sim.caustics`) | 2D water-refraction/caustic look from a supplied height field. Fluid evolution, comp lights and physical caustic ray tracing are unsupported. | Registered height/bottom/optional environment assets; declared IOR/depth model. |
| Time Displacement (`ec.time.timedisplacement`) | Image-driven temporal displacement. Initial operation has a documented small bin count; donor 64/65-way and unbounded HDR-driven time selection are not admitted. | Explicit finite offset range, normalized guide and frame availability. |
| Card Wipe (`ec.transition.cardwipe`) | Bounded card-flip wipe. General 3D camera/lighting, dense grids and image-gradient-driven scene behavior are unsupported. | Explicit grid, front/back layers, ordering and finite progress. |

### Extending the design ledger during implementation

Use each canonical capability task to produce a concrete descriptor, parameter migration, actual FFmpeg graph, component closure and meaningful CI evidence. Shared variants have separate admission fixtures where their precision, timing, input or resource behavior differs. Update implementation/runtime status only with artifact hashes and actual results; do not relabel a source inspection as a passing media test. An unsupported row retains a precise discovery reason and any separately named alternative. The full initial release still requires every admitted baseline workflow; the first vertical slice does not replace it.


