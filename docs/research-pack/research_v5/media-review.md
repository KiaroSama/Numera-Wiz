# Numera Wiz v5 media-contract review

## Applied correction status

The findings below describe the reviewed v4 baseline. The initial bounded corrections were applied to the assigned v5 files: `mandate-media.md`, `filmcraft-functional-mapping-final.csv`, `effectcraft-functional-mapping-final.csv` and their two per-donor Markdown explanations. The initial six corrected FilmCraft source IDs are `color_emboss`, `edge_glow`, `alpha_glow`, `four_color_gradient`, `ramp` and `gradient`. The initial shared EffectCraft route corrections cover `ec.stylize.coloremboss`, `ec.generate.fourcolor` and `ec.generate.gradientramp`. Row counts remain 303 and 307; IDs, design dispositions and unexecuted runtime states are unchanged. The FilmCraft introductory assurance of review completeness was replaced with an explicit implementation-time source/schema/route verification obligation. The bounded color-family followup below records the additional assigned corrections.

The media chapter now contains the bounded immediate-control acknowledgment profile and native clock-relative admission rules, each with focused CI gates. It preserves the earlier NUT timestamp/profile, variable-blur guide, Cairo/drawvg and source-frame-correspondence requirements. It also states the owner's GPU preference explicitly: default `Auto` prefers qualified GPU stage routes when semantics and objective/resource constraints permit; `CpuOnly` performs no hardware media-device initialization; named `GpuRequired` stages cannot fall back silently.

Integration is complete: the aggregate coverage ledger, canonical alias semantics, compact donor index and assembled mandate were regenerated from the corrected v5 inputs. The subsequent bounded light/noise review and its shared-alpha/sampling corrections are recorded in `research_v5/light-noise-review.md`; the full resolution ledger covers both reviews. The `research_v4` references below identify historical reviewed text, not current companion paths or competing instructions. Static CSV/document checks verified row counts and protected IDs/dispositions/statuses; no app/media build, execution, benchmark or test was performed.

## Bounded followup: remaining color-family rows

The additional assigned scan covered all 22 FilmCraft rows that still had `recipe_family=color`, plus the 23 EffectCraft source rows reached through their 21 alias records. This was a finite consistency check against the existing source descriptions, schemas, route/resource fields and pinned archive; it was not a fresh repository-wide research pass. `mandate-media.md` was not edited during this followup.

All 22 FilmCraft schemas inherited the same unrelated list of typed colors, channel choices, exposure, gamma and curves. The suffix was removed while retaining each operation's actual controls. Source-specific gamma remains in gamma correction, levels and Cineon conversion; removing the inherited suffix does not remove those features. Tint's endpoint colors now explicitly preserve original alpha, matching the inspected RGB-only endpoint evaluation in the archived [FilmCraft tint implementation](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/gpufx.rs#L658-L669).

| Additional concrete defect | Applied correction and focused CI gate |
|---|---|
| `unsharp_mask` split/blur route contradicted its pointwise/LUT difference and no-neighborhood resource rule. | Spatial residual contract, one admitted blur branch and bounded intermediate planes; below/above-threshold residual fixtures, flat-field/zero-amount identity and alpha/edge cases. Native zero-threshold shortcut must match the descriptor's kernel and precision. |
| `find_edges` named Sobel/Canny while inheriting LUT-only difference language and forbidding neighboring samples. | Separate named Canny/Sobel variants with detector-specific controls, preserved alpha and native scratch-buffer or fixed-neighborhood budget. Fixtures distinguish a uniform field from translated edges and verify each detector independently. |
| `emboss` named directional convolution while inheriting a pointwise color-transfer/resource contract. | Declared luminance, signed directional response, midpoint bias, original mix and preserved alpha; finite kernel or two-offset variant. Test neutral midpoint on flat/zero-contrast input, opposite-direction response, source-mix identity and boundaries. |
| `strobe` period/duty schema had no temporal gate in its actual color/LUT route; “duty seconds” was ambiguous. | Clip-time periodic gate with positive seconds period, dimensionless duty or explicitly mapped on-duration seconds, phase and half-open boundaries. Preserve PTS/frame count; test boundaries, negative phase, nonzero origin, seek/restart, duty zero/one and qualified seeded variants. |
| `invert` admitted alpha inversion but its generic RGB/LUT route did not implement alpha processing. | Explicit native component/alpha negate or bounded RGB/alpha expressions, output-alpha and premultiplication policy. Test RGB-only alpha preservation, independent alpha inversion, unselected components, zero amount and zero/partial/full alpha. |

Source evidence for spatial and temporal dependencies is the archived [FilmCraft edges/emboss/strobe implementation](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/effects.rs#L167-L236), [unsharp parameter lowering](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/gpufx.rs#L364-L368) and [RGB/alpha inversion](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/gpufx.rs#L750-L773). Native contracts were checked in the pinned manual sections for [unsharp](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L24520-L24605), [Canny edges](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L13309-L13365), [Sobel](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L22842-L22861), [convolution](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L10527-L10607), [component/alpha negate](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L18666-L18694) and [geq coordinates/time/alpha](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15087-L15234). These demonstrate the stated dependencies and available primitives, not executed graph correctness.

Exact additional FilmCraft row IDs changed: `brightness_contrast`, `tint`, `color_balance`, `leave_color`, `change_to_color`, `unsharp_mask`, `black_white`, `color_pass`, `gamma_correction`, `invert`, `levels`, `proc_amp`, `extract`, `posterize`, `find_edges`, `emboss`, `strobe`, `asc_cdl`, `video_limiter`, `channel_mix`, `color_replace`, `cineon_converter`.

Exact additional EffectCraft row IDs changed: `ec.blur.unsharp`, `ec.stylize.findedges`, `ec.stylize.emboss`, `ec.stylize.strobe`, `ec.channel.invert`, `ec.color.brightnesscontrast`. The first five share the corrected canonical routes/fixtures; brightness/contrast loses unrelated optional gamma that was absent from its source controls (`brightness`, `contrast`, `useLegacy`). The other inspected related EffectCraft rows retain their existing operation-specific contracts. No additional unsupported features or rendering engine were introduced.

After this followup, 18 FilmCraft rows remain in the color recipe family; the four spatial/temporal rows moved to descriptive recipe families. There are cumulatively 28 FilmCraft and 9 EffectCraft rows changed by this media subtask relative to v4. Source row counts, target IDs, design dispositions and runtime states remain unchanged. Regenerate derived coverage and any recipe-family summaries from the current CSVs; existing aliases still identify the same canonical capabilities. This finite review does not certify every retained route as complete or executed.

## Scope and evidence status

This is a finite review of the v4 FFmpeg planner/preview contract and the final donor route ledgers. It checks concrete inconsistencies that remain after the earlier 610-row reconciliation. It does not claim that every donor row was independently executed or that a missing implementation is itself a specification defect.

The review read the pinned source and manual files already present in the research archive. Attempts to reopen the official GitHub URLs through the web tool returned `DisabledError`; therefore this report does **not** represent fresh network verification. The source links below identify the exact archived revision being discussed. No application build, media execution, benchmark or test was run locally. All suggested runtime assertions belong in GitHub CI, with owner GPU qualification remaining a separate local run of CI-produced binaries.

Pinned evidence:

- FilmCraft: `d643a886650bfbf16f24762274054575aa1e8f20`.
- EffectCraft: `9db7f3b21700fd821f215782ed1c82afe077880f`.
- FFmpeg: `946fcce07b6dcd0331c8cc609192aeff5e1924f8`.

The first three findings concern actual inconsistent or overbroad recipe text. The final two are implementation-contract gaps: the proposed architecture can work, but those paragraphs do not yet constrain a necessary behavior sufficiently.

## M5-M01 — Color Emboss still inherits a color-only route

**Classification:** confirmed route/schema/resource inconsistency; correct before treating this canonical descriptor as settled.

**Locations:** `research_v4/filmcraft-functional-mapping-final.csv`, row keyed `color_emboss`, physical line 157; its copy in `research_v4/donor-effects-coverage-v4.csv`; alias row `ec.stylize.coloremboss` in `research_v4/cross-donor-capability-aliases.csv`, line 251. The canonical capability is `nw.video.relief.color_emboss`.

The FilmCraft row explicitly requires directional relief and a “directional convolution/residual,” but assigns `recipe_family=color`. Its route permits a parameter-generated 1D/3D LUT, its difference field calls the operation a static color transform, and its resource rule forbids neighborhood scans. It also inherits exposure/gamma/curve schema language unrelated to the stated relief controls.

A color LUT cannot be the complete implementation of directional spatial relief: equal center colors with different neighboring gradients must be able to produce different output. The source implementation samples the image on opposite sides of the current coordinate, calculates a luminance difference and adds the signed response to the original color. This is spatial processing, even though the output retains color. See the archived `research_v3/requirements-filmcraft-crates__render__src__vfx__stylize.rs`, lines 73–94, and the [pinned FilmCraft implementation](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/stylize.rs#L73-L94).

The alias ledger joins this row to EffectCraft's Color Emboss. The latter already has a small directional convolution route in `research_v4/effectcraft-functional-mapping-final.csv`, line 261. The alias identity is reasonable; the two route contracts must be reconciled instead of preserving the FilmCraft color-template text.

**Minimal correction:** use a spatial-relief lowering owner. Admit a small native `convolution` kernel for the discrete-direction/kernel variant, or an explicitly bounded two-offset `geq` sampling expression for continuous direction/distance. Define the signed residual, color domain, contrast, source mix, alpha preservation and edge sampling. A sampled expression is allowed to read the fixed neighboring positions; it must not turn into an unbounded neighborhood loop. Declare the actual supported pixel formats and precision instead of assuming that every convolution route is floating-point.

Suggested replacement resource rule: “Bound the number of source samples per output pixel and the number of retained planes. Use a native small kernel or a fixed two-offset expression; reuse the source branch and preserve alpha. A color-only LUT is not a complete relief lowering.”

The pinned [convolution manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L10527-L10607) describes finite square/row/column kernels, divisors and bias. The [geq manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15087-L15234) documents coordinate-based RGB samples, interpolation, edge clipping and explicit alpha expressions. These are viable building blocks, not evidence that any proposed complete graph has passed qualification.

**Boundary:** preserve the useful directional color-relief function. Do not require the donor's precise transfer functions, default numbers or pixel identity. Do not replace the whole capability with a grayscale emboss preset.

**Meaningful CI assertion:** synthesize equal center RGB values with different surrounding gradients and require different relief output; verify a flat field has zero directional residual; reverse direction and check the residual sign under an unclipped test range; check zero-relief/source-mix identity, alpha preservation and boundary behavior. A test that merely checks the presence of `convolution` in argv is insufficient.

## M5-M02 — Edge Glow and Alpha Glow lack their defining input masks

**Classification:** confirmed route under-specification relative to the existing source-informed target functions. These are straightforward FFmpeg composition tasks, not unsupported features.

Both FilmCraft rows inherit the same generic route: generate a small number of authored analytic/vector light masks, blur them, then add/screen them, with optional source-highlight thresholding. That route does not specify the defining source-dependent mask for either effect. The schema for Edge Glow mentions Sobel, but the authoritative route and its filter evidence omit that stage. Alpha Glow does not even specify alpha extraction or the behind-layer composition required by its source-informed function.

### Edge Glow

**Locations:** `research_v4/filmcraft-functional-mapping-final.csv`, `source_id=edge_glow`, line 149; the corresponding combined-ledger row; canonical `nw.video.light.edge_glow`.

The archived `research_v3/requirements-filmcraft-crates__render__src__vfx__lights.rs`, lines 75–120, explicitly constructs a Sobel edge-strength field, thresholds it, forms an edge core and blurred halo, then colors/composites the light. See the [pinned FilmCraft implementation](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/lights.rs#L75-L120). Thresholding highlights is not equivalent: a uniform bright region has no interior image edge.

**Minimal correction:** specify `declared source luminance/alpha treatment → native sobel or qualified finite gradient kernel → threshold/soft knee → optional edge-width stage → split core/blurred halo → tint/intensity → explicit source/glow-only composition`. Remove irrelevant inherited light-position/seed controls unless a separately named variant actually uses them. Add the pinned [Sobel documentation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L22842-L22861) to this row's evidence. Explicitly avoid processing alpha accidentally through Sobel's default all-plane mask.

**Boundary:** keep the selected edge detector and working domain explicit. Native Sobel does not by itself prove donor mathematical identity, HDR range, alpha semantics or a specific edge-width implementation.

**Meaningful CI assertion:** a uniform bright image has no interior edge halo, while a controlled dark/bright boundary does; translating the boundary translates the light; zero intensity is identity; glow-only suppresses the source; radius alters halo extent without turning the effect into a highlight glow.

### Alpha Glow

**Locations:** `research_v4/filmcraft-functional-mapping-final.csv`, `source_id=alpha_glow`, line 181; corresponding combined-ledger row; canonical `nw.video.light.alpha_glow`.

The archived `lights.rs`, lines 351–393, extracts and blurs source alpha, maps coverage to start/end colors and composites the original foreground **over** the glow. See the [pinned implementation](https://github.com/storytold/filmcraft/blob/d643a886650bfbf16f24762274054575aa1e8f20/crates/render/src/vfx/lights.rs#L351-L393). An authored light mask or RGB highlight threshold cannot substitute for arbitrary alpha geometry.

**Minimal correction:** specify `source alpha → bounded padding/edge policy → native blur → declared coverage/fade function → start/end color and glow alpha → original-over-glow`. Use `alphaextract` or an equivalent qualified native stage; choose behavior for alpha-less inputs explicitly. Declare whether the working canvas expands and how the result is cropped back. Front/glow-only variants, if retained, require their own explicit composition semantics. The [alphaextract manual](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L8540-L8557) confirms the relevant native primitive. The existing blur/composite primitives remain useful.

**Boundary:** the EffectCraft ordinary Glow alias is already marked `distinct_context_shared_primitive`; preserve that distinction. Sharing blur/composite machinery does not make highlight masks and alpha masks interchangeable. Do not introduce a new capability merely to repair the existing row.

**Meaningful CI assertion:** identical RGB planes with different alpha silhouettes produce different halos; opaque foreground pixels retain their original color for the behind-glow mode; transparent RGB values do not create RGB-driven halos; zero intensity is identity; edge-of-canvas and alpha-less input policies match the descriptor.

## M5-M03 — Spatial generators retain an inadmissible LUT alternative

**Classification:** confirmed overbroad alternative in the route text, not an assertion that these generators are impossible.

**Locations:** FilmCraft `four_color_gradient`, `ramp` and `gradient`, physical CSV lines 45, 46 and 142, plus combined-ledger copies. The alias ledger links the four-color and general-gradient rows to corresponding EffectCraft spatial generators.

These rows inherit the same color-transform/LUT template as Color Emboss. Their schema fields mention spatial points or analytic fields, so a coordinate-dependent expression is a valid intended route. However, a 1D/3D color LUT alone cannot create different colors at different positions from a spatially uniform input. The route should not leave that option admissible for the complete generator. The EffectCraft Gradient Ramp row already specifies native `gradients` or coordinate-based `geq`, which is the appropriate level of specificity.

**Minimal correction:** assign a spatial-generator lowering owner. Define a finite source or explicitly reused input canvas with dimensions, cadence, duration, color/alpha and timestamps. Use the supported native `gradients` modes or a bounded `geq` field using coordinates; blend with the original when requested. A LUT may transform the generated colors afterward, but cannot replace coordinate generation. Remove inherited exposure/gamma/curve parameters unless intentionally exposed by a separate color stage. Include the selected radial/reflected/diamond formula and coincident-point rule in the descriptor; avoid implying all named shapes come directly from native `gradients`.

Evidence: pinned [gradients source documentation](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L30123-L30176) and [geq coordinate variables](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/doc/filters.texi#L15087-L15234).

**Meaningful CI assertion:** a uniform source becomes a spatially varying field with the chosen endpoints; authored positions and midpoint have an observable effect; coincident endpoints are deterministic and finite; animation evaluates at project time; duration and packet PTS are finite and correct.

## M5-M04 — Acknowledgment needs a constrained FFmpeg CLI control profile

**Classification:** qualification/implementation-contract gap. The current text correctly warns about polling and permits a generation restart; it does not promise an impossible general API.

**Location:** `research_v4/mandate-media.md`, line 79; main mandate line 773, “Give commands exact filter instance targets, ordering, acknowledgment and a latest-value policy.”

The pinned FFmpeg CLI is not a correlated request/response command protocol:

- Its `c`/`C` parser accepts bounded text fields, with 63 bytes for target and 255 bytes each for command/argument. It waits for a line terminator while reading the command.
- The keyboard check has a 100 ms minimum interval, but the outer loop calls it after `sch_wait(..., stats_period, ...)`; the default `stats_period` is 500,000 microseconds. The 100 ms guard is not a 100 ms response guarantee.
- Immediate commands print a graph-index/return-code response without an application request ID. A not-yet-created graph returns without such a response. Queued timed commands have different reporting and do not provide a later applied acknowledgment for each application request.

Evidence: archived `research_v2/ffmpeg-upstream/fftools_ffmpeg.c`, lines 826–867 and 906–916; `research_v3/ffmpeg-source/fftools_ffmpeg_filter.c`, lines 2321–2350; `research_v3/ffmpeg-source/fftools_ffmpeg_opt.c`, line 81. Official pinned [CLI control source](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/fftools/ffmpeg.c#L826-L867), [filter command dispatch](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/fftools/ffmpeg_filter.c#L2321-L2350), and [polling configuration](https://github.com/FFmpeg/FFmpeg/blob/946fcce07b6dcd0331c8cc609192aeff5e1924f8/fftools/ffmpeg_opt.c#L81).

**Minimal correction:** select one qualified profile for live updates: unique filter-instance targets, immediate commands, one outstanding command per process when stderr response correlation is used, complete line writes, bounded fields, expected graph responses, timeout/generation rejection and a restart fallback. Record the effective polling configuration. Treat a queued command as queued, not audibly applied; rendered automation remains an immutable graph/time contract. Unsupported live controls, including any pan form without a qualified runtime command, use the already-authorized generation restart path.

**Boundary:** do not add a C library renderer or switch away from Rust merely to obtain command acknowledgment. Windows private stdin pipes are supported by the inspected FFmpeg CLI; there is no basis here to prohibit that route. An immediate command's successful return is also not proof that the resulting samples have already reached the output device.

**Meaningful CI assertion:** send two coalescible updates while only one is outstanding, delay the expected response, then seek/restart; no old response may acknowledge the new generation. Exercise graph-not-ready, malformed/overlong request rejection, partial writer failure, blocked media output and shutdown. Verify rendered output eventually reflects the accepted update; measure control-to-audible latency separately from command acceptance.

## M5-M05 — Native playback needs an explicit future-frame admission rule

**Classification:** implementation-contract gap, not a demonstrated deadlock in an implemented program.

**Location:** `research_v4/mandate-media.md`, line 105; main mandate line 799. Browser playback separately has an explicit bounded future-playback policy at lines 107–109, but the native NUT paragraph only states bounded presentation queues, obsolete-frame dropping and an audio-backpressure deadline.

An obsolete or late video frame can be discarded. A future frame is not obsolete merely because a three-frame queue is full. Without a clock-relative admission rule, a fast video-only producer could consume the entire source and continually replace future frames; with interleaved audio/video, blocking before the next required audio packet also needs an explicit bounded resolution. Small Rust vectors alone do not define pacing or bound the producer's retained buffers.

**Minimal correction:** apply a finite, clock-relative render-ahead window to native audio/video and video-only playback. State exactly which frames may be dropped (old generation or too late for presentation), what happens to future frames under backpressure, and how paused/no-demand consumers stop production. Bound and observe demux staging, sockets/pipes, FFmpeg queues and consumer queues. Qualify the selected interleaving/pacing policy with silence, long gaps and unequal stream EOF. Do not assume `-re`, a byte cap or the audio device clock alone proves the whole condition.

**Boundary:** retain the one-producer NUT architecture unless CI demonstrates it cannot meet the declared bounds. This finding calls for a missing state/limit rule, not an additional renderer or an unlimited buffer.

**Meaningful CI assertion:** use a fast-to-generate long video-only source and a deliberately slow virtual presentation clock. After a small elapsed test interval, the final source frame must not already replace all queued future frames; retained memory and producer advancement stay within the chosen horizon. Repeat with sparse/absent audio, audio-first/video-first packet order, pause, seek, stalled consumer and unequal EOF. Confirm an allowed late-frame drop does not change project time or starve audio.

## Changes that should not be reopened without new evidence

- The motion-morph proposal is already labeled as an unexecuted bilateral/multi-frame qualification problem with a release gate. This review found no new proof that justifies declaring it solved or categorically impossible.
- The inspected rawvideo encoder uses packed-image sizing/copying with alignment 1; the restricted packed RGBA payload size is not contradicted by hidden row padding in that source.
- `drawvg` being configured by script/file does not forbid time-dependent scripts. Its output-domain limitations are already addressed by prior review.
- Large conceptual unsupported families must not be readmitted simply because an expression language can approximate them. No verified FFmpeg-native correction for the existing Cryptomatte/ARAP/scene-tracking exclusions was established in this finite review.
- Do not treat the number 610 as a runtime completeness metric. Maintain the existing distinctions between source-row inventory, canonical feature contract, implemented operation and qualified packaged availability.

## Recommended patch boundary

Repair the six FilmCraft rows identified above and their generated combined-ledger copies, keeping existing canonical IDs and source provenance. Reconcile shared canonical lowering with the already better-specified EffectCraft rows. Add the two short preview/control rules to the media contract and their CI acceptance cases. Regenerate derived tables only after the corrected canonical contracts agree; do not create another independent source of truth or claim runtime success from this document review.
